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
    Participation,
    Admitted,
    GroupSettled,
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
    pub participation: Vec<crate::Participation>,
}

pub trait ChatListener: Send + Sync {
    fn on_event(&self, event: ChatEvent) -> Result<(), String>;
}

#[cfg(test)]
type MemoryPublicationHook = Box<dyn FnOnce(&std::path::Path) -> Result<(), Error> + Send>;

#[derive(Default)]
pub(crate) struct Control {
    cancelled: Mutex<bool>,
    wake: Condvar,
    signal: tokio::sync::Notify,
    #[cfg(test)]
    pub(crate) memory_publication: Mutex<Option<MemoryPublicationHook>>,
}

impl Control {
    pub(crate) fn cancel(&self) {
        *self.cancelled.lock().expect("cancellation lock") = true;
        self.wake.notify_all();
        self.signal.notify_one();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        *self.cancelled.lock().expect("cancellation lock")
    }

    pub(crate) async fn cancelled(&self) {
        if !self.is_cancelled() {
            self.signal.notified().await;
        }
    }

    pub(crate) fn while_active<T>(&self, operation: impl FnOnce() -> T) -> Option<T> {
        let cancelled = self.cancelled.lock().expect("cancellation lock");
        if *cancelled { None } else { Some(operation()) }
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

pub(crate) struct RoomInput<'a> {
    pub room: &'a str,
    pub text: &'a str,
    pub mentions: &'a [String],
    pub images: &'a [String],
    pub separate_tasks: bool,
    pub steer: bool,
}

pub struct ChatSubmission {
    pub message: Message,
    pub tasks: Vec<Arc<ChatTask>>,
    pub group: Option<crate::GroupTask>,
    pub participation: Vec<crate::Participation>,
}

pub struct ChatTask {
    pub id: String,
    pub messages: Vec<Message>,
    control: Arc<Control>,
    cancel_on_drop: bool,
}

impl ChatTask {
    pub fn cancel(&self) {
        self.control.cancel();
    }
}

impl Drop for ChatTask {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.cancel();
        }
    }
}

#[derive(Default)]
pub(crate) struct Chats {
    queues: Mutex<HashMap<String, Arc<queue::Queue>>>,
    pub(crate) groups: Mutex<Vec<std::sync::Weak<crate::participation::worker::GroupState>>>,
}

impl Chats {
    pub(crate) fn serialized<T>(
        &self,
        operation: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        let _queues = self.queues.lock().expect("generation queues lock");
        operation()
    }

    pub(crate) fn task_report(
        self: &Arc<Self>,
        db: Arc<Database>,
        root: &std::path::Path,
        id: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<(), Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        if let Some(turn) = db.enqueue_task_report(id)? {
            let launch = (|| {
                let persona = crate::config::load(root)?
                    .personas
                    .into_iter()
                    .find(|p| p.id == turn.persona_id)
                    .ok_or(Error::UnknownPersona)?;
                let queue = queues.entry(persona.id.clone()).or_default().clone();
                self.launch(
                    db.clone(),
                    persona,
                    listener.clone(),
                    queue,
                    turn.clone(),
                    None,
                )
            })();
            match launch {
                // Report work belongs to the manager, not a temporary frontend handle.
                Ok(task) => drop(task),
                Err(error) => {
                    finish_pending(
                        &db,
                        &turn,
                        &listener,
                        MessageStatus::Failed,
                        Some(error.to_string()),
                    );
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn image_report(
        self: &Arc<Self>,
        db: Arc<Database>,
        root: &std::path::Path,
        id: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<(), Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        if let Some(turn) = db.enqueue_image_report(id)? {
            let launch = (|| {
                let persona = crate::config::load(root)?
                    .personas
                    .into_iter()
                    .find(|p| p.id == turn.persona_id)
                    .ok_or(Error::UnknownPersona)?;
                let queue = queues.entry(persona.id.clone()).or_default().clone();
                self.launch(
                    db.clone(),
                    persona,
                    listener.clone(),
                    queue,
                    turn.clone(),
                    None,
                )
            })();
            match launch {
                // Report work belongs to the manager, not a temporary frontend handle.
                Ok(task) => drop(task),
                Err(error) => {
                    finish_pending(
                        &db,
                        &turn,
                        &listener,
                        MessageStatus::Failed,
                        Some(error.to_string()),
                    );
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn start_maintenance(
        self: &Arc<Self>,
        db: Arc<Database>,
        root: std::path::PathBuf,
        load: impl FnOnce() -> Result<Persona, Error>,
    ) -> Result<Arc<crate::MaintenanceTask>, Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        let persona = load()?;
        let job = db.begin_maintenance(&persona)?;
        let control = Arc::new(Control::default());
        let latest = Arc::new(Mutex::new(crate::MaintenanceStatus {
            id: job.id.clone(),
            persona_id: persona.id.clone(),
            status: "running".into(),
            detail: "Reviewing memory before starting a fresh context…".into(),
            memory_saved: false,
            memory_reviewed: db.memory_reviewed(&job.id)?,
        }));
        let task = Arc::new(crate::MaintenanceTask {
            id: job.id.clone(),
            control: control.clone(),
            latest: latest.clone(),
        });
        let reservation = queues
            .entry(persona.id.clone())
            .or_default()
            .clone()
            .reserve(&job.id, &control);
        let wake = Arc::downgrade(self);
        let recovery_db = db.clone();
        let id = job.id.clone();
        if std::thread::Builder::new()
            .name("pebble-memory".into())
            .spawn(move || {
                if !reservation.wait(&control) {
                    control.cancel();
                }
                crate::maintenance::worker(db, root, persona, job, control, latest);
                drop(reservation);
                if let Some(chats) = wake.upgrade() {
                    chats.wake_groups();
                }
            })
            .is_err()
        {
            recovery_db.finish_maintenance(&id, "failed", "Could not start memory maintenance.")?;
            return Err(Error::Generation(
                "Could not start memory maintenance.".into(),
            ));
        }
        Ok(task)
    }

    pub(crate) fn settings(
        &self,
        db: &Database,
        persona: Option<&str>,
        save: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let _queues = self.queues.lock().expect("generation queues lock");
        db.ensure_idle(persona)?;
        db.ensure_decisions_idle(persona)?;
        save()
    }
    pub(crate) fn retry(
        self: &Arc<Self>,
        db: Arc<Database>,
        load: impl FnOnce() -> Result<Persona, Error>,
        reply: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<Arc<ChatTask>, Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        let persona = load()?;
        let queue = queues.entry(persona.id.clone()).or_default().clone();
        let turn = db.enqueue_retry(&persona, reply)?;
        self.launch(db, persona, listener, queue, turn, None)
    }
    pub(crate) fn start(
        self: &Arc<Self>,
        db: Arc<Database>,
        load: impl FnOnce() -> Result<Persona, Error>,
        prompt: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<Arc<ChatTask>, Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        let persona = load()?;
        let queue = queues.entry(persona.id.clone()).or_default().clone();
        let turn = db.enqueue_turn(&persona, prompt)?;
        self.launch(db, persona, listener, queue, turn, None)
    }
    pub(crate) fn start_room(
        self: &Arc<Self>,
        db: Arc<Database>,
        load: impl FnOnce() -> Result<crate::Workspace, Error>,
        input: RoomInput<'_>,
        listener: Arc<dyn ChatListener>,
    ) -> Result<ChatSubmission, Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        let workspace = load()?;
        let (message, turns, decisions) = db.enqueue_input(&workspace.personas, input)?;
        let superseded = db.steered_calls(&message.id)?;
        for queue in queues.values() {
            queue.cancel_calls(&superseded);
        }
        let mut tasks = Vec::new();
        for turn in turns {
            let persona = workspace
                .personas
                .iter()
                .find(|p| p.id == turn.persona_id)
                .expect("validated persona")
                .clone();
            let queue = queues.entry(persona.id.clone()).or_default().clone();
            // Each accepted call is finalized even if a worker cannot start. Earlier workers remain owned.
            match self.launch(
                db.clone(),
                persona,
                listener.clone(),
                queue,
                turn.clone(),
                None,
            ) {
                Ok(task) => tasks.push(task),
                Err(error) => {
                    finish_pending(
                        &db,
                        &turn,
                        &listener,
                        MessageStatus::Failed,
                        Some(error.to_string()),
                    );
                }
            }
        }
        drop(queues);
        self.wake_groups();
        let participation = decisions.iter().map(|w| w.decision.clone()).collect();
        let group = crate::participation::start(
            self.clone(),
            db,
            decisions,
            workspace.data_root,
            tasks.clone(),
            listener,
        );
        Ok(ChatSubmission {
            message,
            tasks,
            group,
            participation,
        })
    }
    pub(crate) fn step_group(
        self: &Arc<Self>,
        db: Arc<Database>,
        round: &str,
        root: &std::path::Path,
        group: &Arc<crate::participation::worker::GroupState>,
    ) -> Result<crate::storage::rounds::Step, Error> {
        use crate::storage::rounds::Step;
        let mut queues = self.queues.lock().expect("generation queues lock");
        let personas = crate::config::load(root)?.personas;
        let action = db.step_round(round, &personas)?;
        if let Step::Admit(turn, decision) = action {
            let persona = personas
                .iter()
                .find(|p| p.id == turn.persona_id)
                .ok_or(Error::UnknownPersona)?
                .clone();
            let queue = queues.entry(persona.id.clone()).or_default().clone();
            let (ack, received) = std::sync::mpsc::sync_channel(1);
            let launched = self.launch(
                db.clone(),
                persona,
                group.listener.clone(),
                queue,
                (*turn).clone(),
                Some((group.clone(), decision, ack)),
            );
            drop(queues);
            if launched.is_ok() {
                let _ = received.recv();
            } else {
                finish_pending(
                    &db,
                    &turn,
                    &group.listener,
                    MessageStatus::Failed,
                    Some("Could not start the reply worker.".into()),
                );
                self.wake_groups();
            }
            Ok(Step::Wait)
        } else {
            Ok(action)
        }
    }
    pub(crate) fn wake_groups(&self) {
        let groups = self
            .groups
            .lock()
            .expect("group controls lock")
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .collect::<Vec<_>>();
        for group in groups {
            group.sync_cancelled();
        }
    }
    pub(crate) fn stop_room(&self, db: &Database, room: &str) -> Result<(), Error> {
        let queues = self.queues.lock().expect("generation queues lock");
        let calls = db.stop_room(room)?;
        let ids = db.invalidate_room_images(room, None)?;
        if let Some(manager) = db.image_generation.lock().expect("image manager").upgrade() {
            manager.cancel_ids(&ids);
        }
        for queue in queues.values() {
            queue.cancel_calls(&calls);
        }
        drop(queues);
        self.wake_groups();
        Ok(())
    }
    pub(crate) fn remove_group_member(
        &self,
        db: &Database,
        room: &str,
        persona: &str,
    ) -> Result<crate::Room, Error> {
        let queues = self.queues.lock().expect("generation queues lock");
        let (room, calls) = db.remove_group_member(room, persona)?;
        let ids = db.invalidate_room_images(&room.id, Some(persona))?;
        if let Some(manager) = db.image_generation.lock().expect("image manager").upgrade() {
            manager.cancel_ids(&ids);
        }
        for queue in queues.values() {
            queue.cancel_calls(&calls);
        }
        drop(queues);
        self.wake_groups();
        Ok(room)
    }
    pub(crate) fn cancel_all(&self) {
        for queue in self.queues.lock().expect("generation queues lock").values() {
            queue.cancel_all();
        }
        for group in self
            .groups
            .lock()
            .expect("group controls lock")
            .iter()
            .filter_map(std::sync::Weak::upgrade)
        {
            group.cancel();
        }
    }
    fn launch(
        self: &Arc<Self>,
        db: Arc<Database>,
        persona: Persona,
        listener: Arc<dyn ChatListener>,
        queue: Arc<queue::Queue>,
        turn: Turn,
        registration: Option<(
            Arc<crate::participation::worker::GroupState>,
            String,
            std::sync::mpsc::SyncSender<()>,
        )>,
    ) -> Result<Arc<ChatTask>, Error> {
        let control = Arc::new(Control::default());
        let task = Arc::new(ChatTask {
            id: turn.call_id.clone(),
            messages: vec![turn.user.clone(), turn.reply.clone()],
            control: control.clone(),
            cancel_on_drop: !db.is_task_report(&turn.call_id).unwrap_or(false)
                && !db.is_image_report(&turn.call_id).unwrap_or(false),
        });
        if let Some((group, _, _)) = &registration {
            group.register(task.clone());
        }
        let reservation = queue.reserve(&turn.call_id, &control);
        let wake = Arc::downgrade(self);
        let worker_db = db.clone();
        let worker_turn = turn.clone();
        let spawn = std::thread::Builder::new()
            .name("pebble-chat".into())
            .spawn(move || {
                if let Some((group, decision, ack)) = registration {
                    if !group.admitted(&decision, &worker_turn) {
                        control.cancel();
                    }
                    let _ = ack.send(());
                }
                if !reservation.wait(&control) {
                    finish_pending(
                        &worker_db,
                        &worker_turn,
                        &listener,
                        MessageStatus::Cancelled,
                        None,
                    );
                } else {
                    let ready = reassess_queued(&worker_db, &persona, &worker_turn, &control);
                    let skipped = matches!(ready, Ok(false));
                    match ready.and_then(|allowed| {
                        if allowed {
                            worker_db.activate_turn(&persona, &worker_turn)
                        } else {
                            Err(Error::Generation(
                                "This optional reply is no longer needed.".into(),
                            ))
                        }
                    }) {
                        Ok(turn) => run(worker_db, turn, control, listener, persona),
                        Err(error) => finish_pending(
                            &worker_db,
                            &worker_turn,
                            &listener,
                            if control.is_cancelled() || skipped {
                                MessageStatus::Cancelled
                            } else {
                                MessageStatus::Failed
                            },
                            Some(error.to_string()),
                        ),
                    }
                }
                drop(reservation);
                if let Some(chats) = wake.upgrade() {
                    chats.wake_groups();
                }
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
            if message.status == MessageStatus::Cancelled {
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
        participation: Vec::new(),
    });
}

fn run(
    db: Arc<Database>,
    turn: Turn,
    control: Arc<Control>,
    listener: Arc<dyn ChatListener>,
    persona: Persona,
) {
    let chat = persona.chat.clone();
    let budget = persona.context;
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
            participation: Vec::new(),
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
            generate_structured(&db, &request, &persona, chat.as_ref(), budget, &control)
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
            if let Some(chat) = &chat {
                budget.check_body(&crate::live::request_body(chat, &request, budget.max_output_tokens)?.1)?;
            } else {
                budget.check_demo(&request)?;
            }
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
        if status != MessageStatus::Complete {
            failure = memory_failure(&db, &turn.call_id, failure);
            if structured {
                content.clear();
            }
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
            match message.status {
                MessageStatus::Complete => ChatEventKind::Completed,
                MessageStatus::Cancelled => ChatEventKind::Cancelled,
                _ => ChatEventKind::Failed,
            },
            vec![message],
            failure.unwrap_or_default(),
        ),
        Err(error) => {
            let detail = memory_failure(&db, &turn.call_id, Some(error.to_string()))
                .expect("publication error");
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

fn memory_failure(db: &Database, owner: &str, detail: Option<String>) -> Option<String> {
    if !db.memory_saved(owner).unwrap_or(false) {
        return detail;
    }
    let recovery = if db.memory_reviewed(owner).unwrap_or(false) {
        crate::storage::memory::SAVED_BEFORE_STOP
    } else {
        "Memory was saved, but its result awaits recovery. Reopen Pebble before retrying; no write will be repeated."
    };
    Some(format!(
        "{} {recovery}",
        detail.unwrap_or_else(|| "Reply cancelled.".into())
    ))
}

fn reassess_queued(
    db: &Database,
    persona: &Persona,
    turn: &Turn,
    control: &Arc<Control>,
) -> Result<bool, Error> {
    let Some(work) = db.queued_reassessment(persona, turn)? else {
        return Ok(true);
    };
    let runtime = worker_runtime()?;
    let result=runtime.block_on(async {tokio::select!{biased; _=control.cancelled()=>Err(Error::Generation("Cancelled".into())),result=crate::participation::classify(persona.classifier.as_ref().ok_or_else(||Error::Participation("Participation is no longer configured.".into()))?,&work.input,&work.phase)=>result}});
    let accepted = control.while_active(|| match &result {
        Ok(answer) => db.finish_decision(
            &work.decision.id,
            work.generation,
            if *answer { "selected" } else { "listening" },
            "",
            Some(*answer),
            None,
        ),
        Err(error) => db.finish_decision(
            &work.decision.id,
            work.generation,
            "failed",
            &error.to_string(),
            None,
            None,
        ),
    });
    if accepted.is_none() {
        db.cancel_reassessment(&work.decision.id, &turn.call_id, work.generation)?;
        return Ok(false);
    }
    result
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
    persona: &Persona,
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
    let mut base_request = request.clone();
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    let unused_tool_phase = !db.tool_used(request)?
        && !db.delegation_used(request)?
        && !db.image_generation_used(request)?;
    base_request.memory_tools = request.memory_tools && chat.is_some() && unused_tool_phase;
    base_request.delegation_tools = request.delegation_tools && chat.is_some() && unused_tool_phase;
    if request.image_generation_tools && chat.is_some() && unused_tool_phase {
        let manager = db.image_generation.lock().expect("image manager").upgrade();
        if let Some(manager) = manager {
            base_request.image_generation_backends = manager.available_backends()?;
        }
    }
    base_request.image_generation_tools = request.image_generation_tools
        && chat.is_some()
        && unused_tool_phase
        && !base_request.image_generation_backends.is_empty();
    let mut tool_phase = base_request.memory_tools
        || base_request.delegation_tools
        || base_request.image_generation_tools;
    let mut candidate_count = 0;
    let mut invalid_delegations = 0;
    let runtime = worker_runtime()?;
    // Two argument corrections, one accepted tool phase, and three final candidates.
    loop {
        if control.is_cancelled() {
            return Err(Error::Generation("Cancelled".into()));
        }
        if std::time::Instant::now() >= deadline {
            return Err(Error::Generation(
                "The complete reply operation exceeded 180 seconds.".into(),
            ));
        }
        // Retry input differs only by transient, specific validation feedback.
        // It never includes a rejected candidate or changes persisted frozen items.
        let mut candidate_request = base_request.clone();
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
        if let Some(chat) = chat {
            let body =
                crate::live::request_body(chat, &candidate_request, budget.max_output_tokens)?.1;
            if candidate_request.memory_tools
                || candidate_request.delegation_tools
                || candidate_request.image_generation_tools
            {
                budget.check_chat_start(&body)?;
            } else {
                budget.check_body(&body)?;
            }
        } else {
            budget.check_demo(&candidate_request)?;
        }
        let output = if let Some(chat) = chat {
            runtime.block_on(async {
                tokio::select! {
                    biased;
                    _ = control.cancelled() => Err(Error::Generation("Cancelled".into())),
                    result = tokio::time::timeout(deadline.saturating_duration_since(std::time::Instant::now()),crate::live::generate(chat,&candidate_request,budget.max_output_tokens,control.clone(),|_|Ok(()))) => result.map_err(|_|Error::Generation("The complete reply operation exceeded 180 seconds.".into()))?,
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
        if tool_phase {
            crate::live::output_text(&output)?; // Refusals are never argument-repair candidates.
            db.archive_tool_output(&request.call_id, &output)?;
            match crate::image_generation::tool::calls(&output)? {
                crate::image_generation::tool::Parsed::None => {}
                crate::image_generation::tool::Parsed::Invalid { id, detail } => {
                    if !base_request.image_generation_tools {
                        return Err(crate::image_generation::failure(
                            "Image generation is unavailable in this context.",
                        ));
                    }
                    invalid_delegations += 1;
                    let result = crate::memory::result(&request.protocol, &id, &json!({"ok":false,"error":"invalid_arguments","detail":detail,"instruction":"Correct generate_image with exactly prompt, backend and size {width,height}. No generation was submitted."}).to_string());
                    control
                        .while_active(|| db.reject_delegation_arguments(request, &output, &result))
                        .ok_or_else(|| Error::Generation("Cancelled".into()))??;
                    if invalid_delegations == 3 {
                        return Err(crate::image_generation::failure(&detail));
                    }
                    crate::memory::append(&mut base_request, &output)?;
                    crate::memory::append(&mut base_request, &[result])?;
                    continue;
                }
                crate::image_generation::tool::Parsed::Valid(call) => {
                    if !base_request.image_generation_tools {
                        return Err(crate::image_generation::failure(
                            "Image generation is unavailable in this context.",
                        ));
                    }
                    let manager = db
                        .image_generation
                        .lock()
                        .expect("image manager")
                        .upgrade()
                        .ok_or_else(|| {
                            crate::image_generation::failure("Image manager is closing.")
                        })?;
                    let settings = crate::image_generation::settings::read(&request.image_root)?;
                    let executable = crate::codex::settings::read(&request.image_root)?.executable;
                    let (result, dispatch) = control
                        .while_active(|| {
                            db.prepare_image_generation(
                                request,
                                &call,
                                &output,
                                &settings,
                                &executable,
                            )
                        })
                        .ok_or_else(|| Error::Generation("Cancelled".into()))??;
                    if let Some(id) = dispatch {
                        manager.dispatch(&id)?;
                    }
                    manager.notify();
                    crate::memory::append(&mut base_request, &output)?;
                    crate::memory::append(&mut base_request, &[result])?;
                    base_request.image_generation_tools = false;
                    base_request.memory_tools = false;
                    base_request.delegation_tools = false;
                    tool_phase = false;
                    continue;
                }
            }
            let delegation = crate::codex::tool::calls(&output)?;
            if let crate::codex::tool::ParsedCall::InvalidArguments { id, detail } = &delegation {
                if !base_request.delegation_tools {
                    return Err(crate::codex::failure(
                        "Delegation is unavailable in this context.",
                    ));
                }
                invalid_delegations += 1;
                let result = crate::codex::tool::argument_feedback(
                    &request.protocol,
                    id,
                    detail,
                    3 - invalid_delegations,
                );
                control
                    .while_active(|| db.reject_delegation_arguments(request, &output, &result))
                    .ok_or_else(|| Error::Generation("Cancelled".into()))??;
                if invalid_delegations == 3 {
                    return Err(crate::codex::failure(&format!(
                        "Invalid delegate_task arguments after 3 attempts: {detail}"
                    )));
                }
                crate::memory::append(&mut base_request, &output)?;
                crate::memory::append(&mut base_request, &[result])?;
                continue;
            }
            if let crate::codex::tool::ParsedCall::Valid(call) = &delegation {
                if !base_request.delegation_tools {
                    return Err(crate::codex::failure(
                        "Delegation is unavailable in this context.",
                    ));
                }
                let manager = db
                    .codex
                    .lock()
                    .expect("task manager")
                    .upgrade()
                    .ok_or_else(|| crate::codex::failure("Task manager is closing."))?;
                let (result, dispatch) = control
                    .while_active(|| {
                        db.prepare_delegation(request, call, &output, &manager.settings()?)
                    })
                    .ok_or_else(|| Error::Generation("Cancelled".into()))??;
                if let Some(id) = dispatch {
                    manager.dispatch(&id)?;
                }
                manager.notify();
                crate::memory::append(&mut base_request, &output)?;
                crate::memory::append(&mut base_request, &[result])?;
                base_request.image_generation_tools = false;
                base_request.delegation_tools = false;
                base_request.memory_tools = false;
                tool_phase = false;
                continue;
            }
            let calls = crate::memory::calls(&output)?;
            tool_phase = false;
            base_request.memory_tools = false;
            base_request.image_generation_tools = false;
            base_request.delegation_tools = false;
            if let Some(call) = calls.first() {
                let expected = db.memory_snapshot(&request.call_id)?;
                let result = db.execute_memory(
                    crate::storage::memory::MemoryContext {
                        owner: &request.call_id,
                        persona,
                        session: Some(&request.session_id),
                        protocol: &request.protocol,
                        expected: &expected,
                    },
                    &output,
                    call,
                    control,
                )?;
                crate::memory::check_result(&result)?;
                crate::memory::append(&mut base_request, &output)?;
                crate::memory::append(&mut base_request, &[result])?;
                continue;
            }
        }
        candidate_count += 1;
        // Transport, refusal and truncation errors leave here, without format repair.
        let text = crate::live::output_text(&output)?;
        match crate::structured::candidate(schema, &text) {
            Ok((reply, _)) => return Ok((reply, output)),
            Err(detail) => {
                db.validation_error(&request.call_id, &detail)?;
                if candidate_count == 3 {
                    return Err(Error::Generation(format!(
                        "Response format/schema validation failed after 3 candidates: {detail}"
                    )));
                }
                feedback = Some(detail);
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/responses_support/mod.rs"]
pub(crate) mod memory_responses_support;
#[cfg(test)]
#[path = "../tests/live_support/mod.rs"]
pub(crate) mod memory_test_support;
#[cfg(test)]
mod memory_failure_tests {
    use super::{memory_test_support::*, *};
    use serde_json::{Value, json};
    #[test]
    fn failed_publication_is_visible_before_silent_final_and_retry_replays_failure() {
        for api in ["chat_completions", "responses"] {
            for failure in ["write", "exchange"] {
                let args = json!({"content":"proposed memory"}).to_string();
                let call = if api == "responses" {
                    format!(
                        "data: {}\n\n",
                        json!({"type":"response.completed","response":{"status":"completed","output":[{"id":"failed-memory-item","type":"function_call","status":"completed","call_id":"failed-memory","name":"update_memory","arguments":args}]}})
                    )
                } else {
                    format!(
                        "{}{}data: [DONE]\n\n",
                        event(
                            json!({"tool_calls":[{"index":0,"id":"failed-memory","type":"function","function":{"name":"update_memory","arguments":args}}]}),
                            Value::Null
                        ),
                        event(json!({}), json!("tool_calls"))
                    )
                };
                let mut delayed = stream(call);
                delayed.delay_headers = true;
                let silent = if api == "responses" {
                    super::memory_responses_support::success(
                        "ordinary final without mentioning memory",
                    )
                } else {
                    success("ordinary final without mentioning memory")
                };
                let server = Server::memory(vec![delayed, stream(silent)]);
                let (root, core, id) = isolated();
                configure(root.path(), &server.url, "mock", api, "");
                let (task, rx) = start(&core, &id, "remember a fact");
                server
                    .requests
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
                *task.control.memory_publication.lock().unwrap() =
                    Some(Box::new(move |previous| {
                        Err(crate::file_error(
                            previous,
                            std::io::Error::other(format!("synthetic {failure} publication error")),
                        ))
                    }));
                let event = terminal(&rx).pop().unwrap();
                assert_eq!(event.kind, ChatEventKind::Failed);
                assert!(
                    event.text.contains(&format!("synthetic {failure}")),
                    "{}: {}",
                    api,
                    event.text
                );
                let failed = core.messages(&id).unwrap().pop().unwrap();
                assert!(failed.content.is_empty());
                assert!(
                    failed
                        .failure_detail
                        .contains(&format!("synthetic {failure}"))
                );
                assert!(
                    server.requests.try_recv().is_err(),
                    "failure must stop before the otherwise valid final"
                );
                assert_eq!(
                    core.workspace().unwrap().personas[0].memory,
                    "Synthetic test memory only."
                );
                let request = core.model_request(&task.id).unwrap();
                assert!(
                    request
                        .items
                        .iter()
                        .any(|i| String::from_utf8_lossy(&i.payload).contains("\\\"ok\\\":false"))
                );
                drop(task);
                drop(core);
                let core = reopen(root.path());
                let (_task, rx) = retry(&core, &id, &failed.id);
                assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
                let replay = server
                    .requests
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap()
                    .1;
                assert_eq!(replay["tool_choice"], "none");
                let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
                assert_eq!(
                    db.query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    1
                );
                assert_eq!(
                    db.query_row("SELECT status FROM memory_operations", [], |r| r
                        .get::<_, String>(0))
                        .unwrap(),
                    "failed"
                );
            }
        }
    }
}
