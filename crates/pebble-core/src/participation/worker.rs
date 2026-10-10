use super::{DecisionWork, Participation};
use crate::{
    ChatEvent, ChatEventKind, ChatListener, ChatTask, Persona,
    chat::{Chats, Control},
    storage::Database,
};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Instant;

pub struct GroupTask {
    state: Arc<GroupState>,
}
pub(crate) struct GroupState {
    pub(crate) listener: Arc<dyn ChatListener>,
    db: Arc<Database>,
    room: String,
    message: String,
    controls: Vec<(String, Arc<Control>)>,
    tasks: Mutex<Vec<Arc<ChatTask>>>,
    remaining: AtomicUsize,
}
impl GroupTask {
    pub fn id(&self) -> &str {
        &self.state.message
    }
    pub fn tasks(&self) -> Vec<Arc<ChatTask>> {
        self.state.tasks.lock().expect("group tasks lock").clone()
    }
    pub fn cancel(&self) {
        self.state.cancel();
    }
    pub fn cancel_decision(&self, id: &str) {
        self.state.cancel_decision(id);
    }
}
impl Drop for GroupTask {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl GroupState {
    pub(crate) fn register(&self, task: Arc<ChatTask>) {
        self.tasks.lock().expect("group tasks lock").push(task);
    }
    pub(crate) fn admitted(&self, decision: &str, turn: &crate::history::Turn) -> bool {
        self.listener
            .on_event(ChatEvent {
                run_id: turn.call_id.clone(),
                conversation_id: self.room.clone(),
                persona_id: turn.persona_id.clone(),
                message_id: turn.reply.id.clone(),
                sequence: 0,
                kind: ChatEventKind::Admitted,
                text: String::new(),
                messages: vec![turn.user.clone(), turn.reply.clone()],
                participation: self.db.decision(decision).into_iter().collect(),
            })
            .is_ok()
    }
    fn notify(&self, decision: Participation) -> bool {
        self.listener
            .on_event(ChatEvent {
                run_id: self.message.clone(),
                conversation_id: self.room.clone(),
                persona_id: decision.persona_id.clone(),
                message_id: self.message.clone(),
                sequence: 0,
                kind: ChatEventKind::Participation,
                text: String::new(),
                messages: Vec::new(),
                participation: vec![decision],
            })
            .is_ok()
    }
    fn cancel_decision(&self, id: &str) {
        if let Some((_, control)) = self.controls.iter().find(|(key, _)| key == id) {
            control.cancel();
            if let Ok(decision) = self.db.finish_decision(
                id,
                "cancelled",
                "Participation stopped; mention this character to request a reply.",
                None,
                None,
            ) {
                let _ = self.notify(decision);
            }
        }
    }
    pub(crate) fn cancel_decisions(&self) {
        for (id, _) in &self.controls {
            self.cancel_decision(id);
        }
    }
    pub(crate) fn cancel(&self) {
        self.cancel_decisions();
        for task in self.tasks.lock().expect("group tasks lock").iter() {
            task.cancel();
        }
    }
    fn finished(&self) {
        if self.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
            let event = ChatEvent {
                run_id: self.message.clone(),
                conversation_id: self.room.clone(),
                persona_id: String::new(),
                message_id: self.message.clone(),
                sequence: 0,
                kind: ChatEventKind::GroupSettled,
                text: String::new(),
                messages: Vec::new(),
                participation: Vec::new(),
            };
            if self.listener.on_event(event).is_err() {
                self.cancel();
            }
        }
    }
}

pub(crate) fn start(
    chats: Arc<Chats>,
    db: Arc<Database>,
    work: Vec<DecisionWork>,
    personas: Vec<Persona>,
    tasks: Vec<Arc<ChatTask>>,
    listener: Arc<dyn ChatListener>,
) -> Option<GroupTask> {
    let room = work.first().map(|w| w.decision.conversation_id.clone());
    let pending: Vec<_> = work
        .into_iter()
        .filter(|w| w.decision.status == "pending")
        .collect();
    // A new user post invalidates older undecided participation, never accepted chat calls.
    if let Some(room) = room {
        let groups = chats
            .groups
            .lock()
            .expect("group controls lock")
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|g| g.room == room)
            .collect::<Vec<_>>();
        for group in groups {
            group.cancel_decisions();
        }
    }
    let first = pending.first()?;
    let state = Arc::new(GroupState {
        listener,
        db,
        room: first.decision.conversation_id.clone(),
        message: first.decision.message_id.clone(),
        controls: pending
            .iter()
            .map(|w| (w.decision.id.clone(), Arc::new(Control::default())))
            .collect(),
        tasks: Mutex::new(tasks),
        remaining: AtomicUsize::new(pending.len()),
    });
    {
        let mut groups = chats.groups.lock().expect("group controls lock");
        groups.retain(|g| g.strong_count() > 0);
        groups.push(Arc::downgrade(&state));
    }
    for work in pending {
        let persona = personas
            .iter()
            .find(|p| p.id == work.decision.persona_id)
            .expect("accepted persona")
            .clone();
        let control = state
            .controls
            .iter()
            .find(|(id, _)| id == &work.decision.id)
            .unwrap()
            .1
            .clone();
        let group = state.clone();
        let queues = chats.clone();
        let id = work.decision.id.clone();
        let spawned = std::thread::Builder::new()
            .name("pebble-participation".into())
            .spawn(move || {
                let started = Instant::now();
                let result = (|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| crate::Error::Participation("Could not start participation worker.".into()))?;
                    runtime.block_on(async {
                        tokio::select! {
                            biased;
                            _ = control.cancelled() => Err(crate::Error::Participation("Participation stopped.".into())),
                            result = super::http::classify(persona.classifier.as_ref().expect("configured classifier"), &work.input) => result,
                        }
                    })
                })();
                let elapsed = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                let accepted = control.while_active(|| match result {
                    Ok(true) => queues.admit_participation(group.db.clone(), persona, &work.decision.id, elapsed, &group),
                    Ok(false) => group.db.finish_decision(&work.decision.id, "listening", "", Some(false), Some(elapsed)).map(|_| None),
                    Err(error) => group.db.finish_decision(&work.decision.id, "failed", &error.to_string(), None, Some(elapsed)).map(|_| None),
                });
                // Notify dynamic handles after releasing the cancellation mutex:
                // a listener can synchronously cancel from its callback.
                if let Some(Ok(Some(ack))) = &accepted { let _ = ack.recv(); }
                if let Some(Err(error)) = &accepted {
                    let _ = group.db.finish_decision(&work.decision.id, "failed", &error.to_string(), None, Some(elapsed));
                }
                if accepted.is_none() {
                    let _ = group.db.finish_decision(&work.decision.id, "cancelled", "Participation stopped; mention this character to continue.", None, Some(elapsed));
                }
                if let Ok(decision) = group.db.decision(&work.decision.id) && !group.notify(decision) { group.cancel(); }
                group.finished();
            });
        if spawned.is_err() {
            if let Ok(decision) = state.db.finish_decision(
                &id,
                "failed",
                "Could not start participation worker.",
                None,
                None,
            ) {
                let _ = state.notify(decision);
            }
            state.finished();
        }
    }
    Some(GroupTask { state })
}
