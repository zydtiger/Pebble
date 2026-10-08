use crate::{Error, Persona};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewEventKind {
    Started,
    TextDelta,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct PreviewEvent {
    pub run_id: String,
    pub conversation_id: String,
    pub persona_id: String,
    pub sequence: u64,
    pub kind: PreviewEventKind,
    pub text: String,
}

pub trait PreviewListener: Send + Sync {
    fn on_event(&self, event: PreviewEvent) -> Result<(), String>;
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
            .wait_timeout_while(cancelled, Duration::from_millis(48), |cancelled| {
                !*cancelled
            })
            .expect("cancellation wait");
        *cancelled
    }
}

pub struct PreviewTask {
    pub id: String,
    control: Arc<Control>,
}

impl PreviewTask {
    pub fn cancel(&self) {
        self.control.cancel();
    }
}

impl Drop for PreviewTask {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Default)]
pub(crate) struct Previews {
    active: Mutex<Option<Weak<Control>>>,
}

impl Previews {
    pub(crate) fn start(
        &self,
        persona: Persona,
        prompt: &str,
        listener: Arc<dyn PreviewListener>,
    ) -> Result<PreviewTask, Error> {
        let mut active = self.active.lock().expect("preview lock");
        if active
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|control| !control.finished.load(Ordering::Acquire))
        {
            return Err(Error::Busy);
        }
        let control = Arc::new(Control::default());
        let task = PreviewTask {
            id: Uuid::new_v4().to_string(),
            control: control.clone(),
        };
        let run_id = task.id.clone();
        let text = preview_text(&persona.display_name, prompt);
        std::thread::Builder::new()
            .name("pebble-preview".into())
            .spawn(move || {
                let emit = |sequence, kind, text| {
                    listener.on_event(PreviewEvent {
                        run_id: run_id.clone(),
                        conversation_id: persona.conversation_id.clone(),
                        persona_id: persona.id.clone(),
                        sequence,
                        kind,
                        text,
                    })
                };
                let mut sequence = 0;
                if emit(sequence, PreviewEventKind::Started, String::new()).is_ok() {
                    let chunks: Vec<_> = text.chars().collect();
                    let mut connected = true;
                    let mut cancelled = false;
                    for chunk in chunks.chunks(7) {
                        if control.wait() {
                            cancelled = true;
                            break;
                        }
                        sequence += 1;
                        if emit(
                            sequence,
                            PreviewEventKind::TextDelta,
                            chunk.iter().collect(),
                        )
                        .is_err()
                        {
                            connected = false;
                            break;
                        }
                    }
                    // Make the terminal decision before allowing a subsequent preview.
                    control.finished.store(true, Ordering::Release);
                    if connected {
                        let kind = if cancelled {
                            PreviewEventKind::Cancelled
                        } else {
                            PreviewEventKind::Completed
                        };
                        let _ = emit(sequence + 1, kind, String::new());
                    }
                }
                control.finished.store(true, Ordering::Release);
            })
            .map_err(|error| Error::Preview(error.to_string()))?;
        *active = Some(Arc::downgrade(&task.control));
        Ok(task)
    }
}

fn preview_text(name: &str, prompt: &str) -> String {
    if prompt
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    {
        return format!(
            "你好，我是 {name}。\n\n一颗小石子，一件想分享的小事。我们可以从这里开始，慢慢认识彼此。\n\n这是一段预先写好的界面预览，还没有连接模型，也不会保存聊天内容。"
        );
    }
    format!(
        "I'm {name}.\n\nI like the small things: a song you keep coming back to, a thought you haven't quite found words for, something that made you smile on the way home.\n\nFor now, this is a little preview of our space. When we're ready, we'll have a lot more to talk about."
    )
}
