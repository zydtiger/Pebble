use crate::{Error, Message, MessageStatus, Persona, demo, history::Turn, storage::Database};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
mod queue;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatEventKind {
    Started,
    TextDelta,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ChatEvent {
    pub run_id: String,
    pub conversation_id: String,
    pub persona_id: String,
    pub message_id: String,
    pub sequence: u64,
    pub kind: ChatEventKind,
    pub text: String,
    pub messages: Vec<Message>,
}

pub trait ChatListener: Send + Sync {
    fn on_event(&self, event: ChatEvent) -> Result<(), String>;
}

#[derive(Default)]
pub(crate) struct Control {
    cancelled: Mutex<bool>,
    wake: Condvar,
    signal: tokio::sync::Notify,
}

impl Control {
    fn cancel(&self) {
        *self.cancelled.lock().expect("cancellation lock") = true;
        self.wake.notify_all();
        self.signal.notify_one();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        *self.cancelled.lock().expect("cancellation lock")
    }

    async fn cancelled(&self) {
        if !self.is_cancelled() {
            self.signal.notified().await;
        }
    }

    fn wait(&self) -> bool {
        let cancelled = self.cancelled.lock().expect("cancellation lock");
        let (cancelled, _) = self
            .wake
            .wait_timeout_while(cancelled, Duration::from_millis(24), |value| !*value)
            .expect("cancellation wait");
        *cancelled
    }
}

pub struct ChatTask {
    pub id: String,
    pub messages: Vec<Message>,
    control: Arc<Control>,
}

impl ChatTask {
    pub fn cancel(&self) {
        self.control.cancel();
    }
}

impl Drop for ChatTask {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Default)]
pub(crate) struct Chats {
    queues: Mutex<HashMap<String, Arc<queue::Queue>>>,
}

impl Chats {
    pub(crate) fn serialized<T>(
        &self,
        operation: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        let _queues = self.queues.lock().expect("generation queues lock");
        operation()
    }

    pub(crate) fn fresh_context(
        &self,
        db: &Database,
        load: impl FnOnce() -> Result<Persona, Error>,
    ) -> Result<(), Error> {
        self.serialized(|| db.fresh_context(&load()?))
    }

    pub(crate) fn settings(
        &self,
        db: &Database,
        persona: Option<&str>,
        save: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let _queues = self.queues.lock().expect("generation queues lock");
        db.ensure_idle(persona)?;
        save()
    }
    pub(crate) fn retry(
        &self,
        db: Arc<Database>,
        load: impl FnOnce() -> Result<Persona, Error>,
        reply: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<ChatTask, Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        let persona = load()?;
        let queue = queues.entry(persona.id.clone()).or_default().clone();
        let turn = db.enqueue_retry(&persona, reply)?;
        Self::launch(db, persona, listener, queue, turn)
    }
    pub(crate) fn start(
        &self,
        db: Arc<Database>,
        load: impl FnOnce() -> Result<Persona, Error>,
        prompt: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<ChatTask, Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        let persona = load()?;
        let queue = queues.entry(persona.id.clone()).or_default().clone();
        let turn = db.enqueue_turn(&persona, prompt)?;
        Self::launch(db, persona, listener, queue, turn)
    }
    fn launch(
        db: Arc<Database>,
        persona: Persona,
        listener: Arc<dyn ChatListener>,
        queue: Arc<queue::Queue>,
        turn: Turn,
    ) -> Result<ChatTask, Error> {
        let control = Arc::new(Control::default());
        let task = ChatTask {
            id: turn.call_id.clone(),
            messages: vec![turn.user.clone(), turn.reply.clone()],
            control: control.clone(),
        };
        let reservation = queue.reserve(&turn.call_id, &control);
        let worker_db = db.clone();
        let worker_turn = turn.clone();
        let spawn = std::thread::Builder::new()
            .name("pebble-chat".into())
            .spawn(move || {
                if !reservation.wait(&control) {
                    finish_pending(
                        &worker_db,
                        &worker_turn,
                        &listener,
                        MessageStatus::Cancelled,
                        None,
                    );
                } else {
                    match worker_db.activate_turn(&persona, &worker_turn) {
                        Ok(turn) => run(
                            worker_db,
                            turn,
                            control,
                            listener,
                            persona.chat,
                            persona.context,
                        ),
                        Err(error) => finish_pending(
                            &worker_db,
                            &worker_turn,
                            &listener,
                            MessageStatus::Failed,
                            Some(error.to_string()),
                        ),
                    }
                }
                drop(reservation);
            });
        if let Err(error) = spawn {
            db.finish_pending(&turn, MessageStatus::Failed, Some(&error.to_string()))?;
            return Err(Error::Generation(error.to_string()));
        }
        Ok(task)
    }
}

impl Drop for Chats {
    fn drop(&mut self) {
        for queue in self
            .queues
            .get_mut()
            .expect("generation queues lock")
            .values()
        {
            queue.cancel_all();
        }
    }
}

fn finish_pending(
    db: &Database,
    turn: &Turn,
    listener: &Arc<dyn ChatListener>,
    status: MessageStatus,
    error: Option<String>,
) {
    let result = db.finish_pending(turn, status, error.as_deref());
    let (kind, messages, detail) = match result {
        Ok(message) => (
            if status == MessageStatus::Cancelled {
                ChatEventKind::Cancelled
            } else {
                ChatEventKind::Failed
            },
            vec![message],
            error.unwrap_or_default(),
        ),
        Err(error) => (ChatEventKind::Failed, Vec::new(), error.to_string()),
    };
    let _ = listener.on_event(ChatEvent {
        run_id: turn.call_id.clone(),
        conversation_id: turn.conversation_id.clone(),
        persona_id: turn.persona_id.clone(),
        message_id: turn.reply.id.clone(),
        sequence: 0,
        kind,
        text: detail,
        messages,
    });
}

fn run(
    db: Arc<Database>,
    turn: Turn,
    control: Arc<Control>,
    listener: Arc<dyn ChatListener>,
    chat: Option<crate::ChatConnection>,
    budget: crate::ContextBudget,
) {
    let emit = |sequence, kind, text, messages| {
        listener.on_event(ChatEvent {
            run_id: turn.call_id.clone(),
            conversation_id: turn.conversation_id.clone(),
            persona_id: turn.persona_id.clone(),
            message_id: turn.reply.id.clone(),
            sequence,
            kind,
            text,
            messages,
        })
    };
    let mut connected = emit(
        0,
        ChatEventKind::Started,
        String::new(),
        vec![turn.user.clone(), turn.reply.clone()],
    )
    .is_ok();
    if !connected {
        control.cancel();
    }
    let mut content = String::new();
    let mut sequence = 0;
    let mut status = MessageStatus::Complete;
    let mut failure = None;
    let mut native = None;
    let request = db.request(&turn.call_id);
    let structured = request.as_ref().is_ok_and(|r| r.response_schema.is_some());
    let generated = request.and_then(|request| {
        if structured {
            generate_structured(&db, &request, chat.as_ref(), budget, &control)
                .map(|(reply, output)| {
                    content = reply;
                    Some(output)
                })
        } else {
            let mut checkpoint = |delta: &str| -> Result<(), Error> {
                if !connected || control.is_cancelled() {
                    control.cancel();
                    return Err(Error::Generation("Cancelled".into()));
                }
                let next = format!("{content}{delta}");
                db.checkpoint(&turn, &next)?;
                content = next;
                sequence += 1;
                connected = emit(sequence, ChatEventKind::TextDelta, delta.into(), Vec::new()).is_ok();
                if !connected {
                    control.cancel();
                }
                Ok(())
            };
            budget.check(&request)?;
            if let Some(chat) = chat {
                let runtime = worker_runtime()?;
                runtime.block_on(async {
                    tokio::select! {
                        biased;
                        _ = control.cancelled() => Err(Error::Generation("Cancelled".into())),
                        result = crate::live::generate(&chat, &request, budget.max_output_tokens, control.clone(), &mut checkpoint) => result.map(Some),
                    }
                })
            } else {
                let text = demo::reply(&request)?;
                for chunk in text.chars().collect::<Vec<_>>().chunks(7) {
                    if control.wait() {
                        return Err(Error::Generation("Cancelled".into()));
                    }
                    checkpoint(&chunk.iter().collect::<String>())?;
                }
                Ok(None)
            }
        }
    });
    match generated {
        Ok(output) => native = output,
        Err(_) if control.is_cancelled() => status = MessageStatus::Cancelled,
        Err(error) => {
            status = MessageStatus::Failed;
            failure = Some(error.to_string());
        }
    }
    // Cancellation and acceptance have one linearization point. A cancel observed
    // before publication cannot commit visible text, native output, or state.
    let result = {
        let cancelled = control.cancelled.lock().expect("cancellation lock");
        if *cancelled || !connected {
            status = MessageStatus::Cancelled;
        }
        if structured && status != MessageStatus::Complete {
            content.clear();
        }
        db.finish_turn(
            &turn,
            &content,
            status,
            failure.as_deref(),
            native.as_deref(),
        )
    };
    let (kind, messages, error) = match result {
        Ok(message) => (
            match status {
                MessageStatus::Complete => ChatEventKind::Completed,
                MessageStatus::Cancelled => ChatEventKind::Cancelled,
                _ => ChatEventKind::Failed,
            },
            vec![message],
            failure.unwrap_or_default(),
        ),
        Err(error) => {
            let detail = error.to_string();
            if structured {
                content.clear();
            }
            let messages = db
                .finish_turn(&turn, &content, MessageStatus::Failed, Some(&detail), None)
                .map(|message| vec![message])
                .unwrap_or_default();
            (ChatEventKind::Failed, messages, detail)
        }
    };
    // The queue releases this slot after durable terminal publication.
    if connected {
        let _ = emit(sequence + 1, kind, error, messages);
    }
}

fn worker_runtime() -> Result<tokio::runtime::Runtime, Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Error::Generation("Could not initialize the model worker.".into()))
}

fn generate_structured(
    db: &Database,
    request: &crate::ModelRequest,
    chat: Option<&crate::ChatConnection>,
    budget: crate::ContextBudget,
    control: &Arc<Control>,
) -> Result<(String, Vec<serde_json::Value>), Error> {
    use serde_json::json;
    let schema = request
        .response_schema
        .as_ref()
        .expect("structured session");
    let state = request
        .state_before
        .as_ref()
        .ok_or_else(|| Error::Generation("Missing frozen persona state.".into()))?;
    let mut feedback: Option<String> = None;
    let runtime = worker_runtime()?;
    for attempt in 0..3 {
        if control.is_cancelled() {
            return Err(Error::Generation("Cancelled".into()));
        }
        // Retry input differs only by transient, specific validation feedback.
        // It never includes a rejected candidate or changes persisted frozen items.
        let mut candidate_request = request.clone();
        if let Some(detail) = &feedback {
            let text = format!(
                "Response validation feedback: {detail} Regenerate only the response from the SAME frozen input and current_state. Rejected candidate state was never accepted. Return a complete valid JSON response."
            );
            candidate_request.items.push(crate::ModelItem {
                id: String::new(),
                sequence: i64::MAX,
                payload: serde_json::to_vec(&crate::storage::input_message(
                    &request.protocol,
                    "user",
                    &text,
                ))?,
            });
        }
        budget.check(&candidate_request)?;
        let output = if let Some(chat) = chat {
            runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = control.cancelled() => Err(Error::Generation("Cancelled".into())),
                    result = crate::live::generate(chat,&candidate_request,budget.max_output_tokens,control.clone(),|_|Ok(())) => result,
                }
            })?
        } else {
            let text = demo::reply(request)?;
            // The deterministic demo exercises the same pending/cancel lifecycle.
            for _ in text.chars().collect::<Vec<_>>().chunks(7) {
                if control.wait() {
                    return Err(Error::Generation("Cancelled".into()));
                }
            }
            vec![
                json!({"role":"assistant","content":serde_json::to_string(&json!({"reply":text,"state":state}))?}),
            ]
        };
        // Transport, refusal and truncation errors leave here, without format repair.
        let text = crate::live::output_text(&output)?;
        match crate::structured::candidate(schema, &text) {
            Ok((reply, _)) => return Ok((reply, output)),
            Err(detail) => {
                db.validation_error(&request.call_id, &detail)?;
                if attempt == 2 {
                    return Err(Error::Generation(format!(
                        "Response format/schema validation failed after 3 candidates: {detail}"
                    )));
                }
                feedback = Some(detail);
            }
        }
    }
    unreachable!("bounded candidate loop")
}
