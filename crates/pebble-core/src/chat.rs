use crate::{Error, Message, MessageStatus, Persona, demo, history::Turn, storage::Database};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
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
struct Control {
    cancelled: Mutex<bool>,
    wake: Condvar,
    finished: AtomicBool,
}

impl Control {
    fn cancel(&self) {
        *self.cancelled.lock().expect("cancellation lock") = true;
        self.wake.notify_all();
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
    active: Mutex<Option<Weak<Control>>>,
}

impl Chats {
    pub(crate) fn start(
        &self,
        db: Arc<Database>,
        persona: Persona,
        prompt: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<ChatTask, Error> {
        let mut active = self.active.lock().expect("generation lock");
        if active
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|control| !control.finished.load(Ordering::Acquire))
        {
            return Err(Error::Busy);
        }
        let turn = db.begin_turn(&persona, prompt)?;
        let text = match db
            .request(&turn.call_id)
            .and_then(|request| demo::reply(&request))
        {
            Ok(text) => text,
            Err(error) => {
                db.finish_turn(&turn, "", MessageStatus::Failed, Some(&error.to_string()))?;
                return Err(error);
            }
        };
        let control = Arc::new(Control::default());
        let task = ChatTask {
            id: turn.call_id.clone(),
            messages: vec![turn.user.clone(), turn.reply.clone()],
            control: control.clone(),
        };
        let worker_db = db.clone();
        let worker_turn = turn.clone();
        let spawn = std::thread::Builder::new()
            .name("pebble-demo".into())
            .spawn(move || {
                run(worker_db, worker_turn, control, listener, text);
            });
        if let Err(error) = spawn {
            db.finish_turn(&turn, "", MessageStatus::Failed, Some(&error.to_string()))?;
            return Err(Error::Generation(error.to_string()));
        }
        *active = Some(Arc::downgrade(&task.control));
        Ok(task)
    }
}

fn run(
    db: Arc<Database>,
    turn: Turn,
    control: Arc<Control>,
    listener: Arc<dyn ChatListener>,
    text: String,
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
    let mut content = String::new();
    let mut sequence = 0;
    let mut status = MessageStatus::Complete;
    let mut failure = None;
    for chunk in text.chars().collect::<Vec<_>>().chunks(7) {
        if !connected || control.wait() {
            status = MessageStatus::Cancelled;
            break;
        }
        let delta: String = chunk.iter().collect();
        let next = format!("{content}{delta}");
        if let Err(error) = db.checkpoint(&turn, &next) {
            status = MessageStatus::Failed;
            failure = Some(error.to_string());
            break;
        }
        content = next;
        sequence += 1;
        connected = emit(sequence, ChatEventKind::TextDelta, delta, Vec::new()).is_ok();
    }
    if !connected {
        status = MessageStatus::Cancelled;
    }
    let result = db.finish_turn(&turn, &content, status, failure.as_deref());
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
            let messages = db
                .finish_turn(&turn, &content, MessageStatus::Failed, Some(&detail))
                .map(|message| vec![message])
                .unwrap_or_default();
            (ChatEventKind::Failed, messages, detail)
        }
    };
    // Persist the terminal outcome before another call or the terminal UI event.
    control.finished.store(true, Ordering::Release);
    if connected {
        let _ = emit(sequence + 1, kind, error, messages);
    }
}
