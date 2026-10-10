use super::Control;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};

#[derive(Default)]
pub(super) struct Queue {
    entries: Mutex<VecDeque<(String, Weak<Control>)>>,
}
impl Queue {
    pub(super) fn reserve(self: &Arc<Self>, id: &str, control: &Arc<Control>) -> Reservation {
        self.entries
            .lock()
            .expect("queue lock")
            .push_back((id.into(), Arc::downgrade(control)));
        Reservation {
            queue: self.clone(),
            id: id.into(),
        }
    }
    pub(super) fn cancel_calls(&self, ids: &[String]) {
        for (id, control) in self.entries.lock().expect("queue lock").iter() {
            if ids.contains(id)
                && let Some(control) = control.upgrade()
            {
                control.cancel();
            }
        }
    }
    pub(super) fn cancel_all(&self) {
        for (_, control) in self.entries.lock().expect("queue lock").iter() {
            if let Some(control) = control.upgrade() {
                control.cancel();
            }
        }
    }
}

pub(super) struct Reservation {
    queue: Arc<Queue>,
    id: String,
}
impl Reservation {
    pub(super) fn wait(&self, control: &Control) -> bool {
        loop {
            if control.is_cancelled() {
                return false;
            }
            if self
                .queue
                .entries
                .lock()
                .expect("queue lock")
                .front()
                .is_some_and(|(id, _)| id == &self.id)
            {
                return true;
            }
            if control.wait() {
                return false;
            }
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.queue
            .entries
            .lock()
            .expect("queue lock")
            .retain(|(id, _)| id != &self.id);
    }
}
