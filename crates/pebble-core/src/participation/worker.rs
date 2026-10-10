use super::{DecisionWork, Participation};
use crate::{
    ChatEvent, ChatEventKind, ChatListener, ChatTask, Persona,
    chat::{Chats, Control},
    storage::{Database, rounds::Step},
};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

pub struct GroupTask {
    state: Arc<GroupState>,
}
pub(crate) struct GroupState {
    pub(crate) listener: Arc<dyn ChatListener>,
    db: Arc<Database>,
    room: String,
    message: String,
    controls: Mutex<Vec<(String, i64, Arc<Control>)>>,
    tasks: Mutex<Vec<Arc<ChatTask>>>,
    changed: Mutex<u64>,
    wake: Condvar,
    records: Mutex<Vec<Participation>>,
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
    pub(crate) fn signal(&self) {
        *self.changed.lock().expect("round wake lock") += 1;
        self.wake.notify_all();
    }
    pub(crate) fn register(&self, task: Arc<ChatTask>) {
        self.tasks.lock().expect("group tasks lock").push(task);
    }
    pub(crate) fn sync_cancelled(&self) {
        for (id, generation, control) in self
            .controls
            .lock()
            .expect("classifier controls lock")
            .iter()
        {
            if !self.db.decision_active(id, *generation) {
                control.cancel();
            }
        }
        for task in self.tasks.lock().expect("group tasks lock").iter() {
            if !self.db.call_active(&task.id) {
                task.cancel();
            }
        }
        self.signal();
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
    fn refresh(&self) -> bool {
        let Ok(records) = self.db.participation(&self.room) else {
            return false;
        };
        let records = records
            .into_iter()
            .filter(|r| r.message_id == self.message)
            .collect::<Vec<_>>();
        let previous = self.records.lock().expect("round records lock").clone();
        for record in &records {
            if !previous.contains(record) && !self.notify(record.clone()) {
                return false;
            }
        }
        *self.records.lock().expect("round records lock") = records;
        true
    }
    fn cancel_decision(&self, id: &str) {
        let _ = self.db.cancel_decision(id);
        self.sync_cancelled();
    }
    pub(crate) fn cancel(&self) {
        let _ = self.db.stop_round(&self.message);
        self.sync_cancelled();
    }
    fn settled(&self) {
        let _ = self.listener.on_event(ChatEvent {
            run_id: self.message.clone(),
            conversation_id: self.room.clone(),
            persona_id: String::new(),
            message_id: self.message.clone(),
            sequence: 0,
            kind: ChatEventKind::GroupSettled,
            text: String::new(),
            messages: Vec::new(),
            participation: Vec::new(),
        });
    }
    fn classify(self: &Arc<Self>, work: DecisionWork, persona: Persona) {
        let control = Arc::new(Control::default());
        self.controls
            .lock()
            .expect("classifier controls lock")
            .push((work.decision.id.clone(), work.generation, control.clone()));
        let group = self.clone();
        let id = work.decision.id.clone();
        let generation = work.generation;
        let failed_id = id.clone();
        let spawned=std::thread::Builder::new().name("pebble-participation".into()).spawn(move||{
            let started=Instant::now();
            let result=(||{
                let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|crate::Error::Participation("Could not start participation worker.".into()))?;
                runtime.block_on(async {tokio::select!{biased;_ = control.cancelled()=>Err(crate::Error::Participation("Participation stopped.".into())),result=super::http::classify(persona.classifier.as_ref().expect("configured classifier"),&work.input,&work.phase)=>result}})
            })();
            let elapsed=started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            let _=control.while_active(||match result {
                Ok(answer)=>group.db.finish_decision(&id,generation,if answer {"selected"} else {"listening"},"",Some(answer),Some(elapsed)),
                Err(error)=>group.db.finish_decision(&id,generation,"failed",&error.to_string(),None,Some(elapsed)),
            });
            group.signal();
        });
        if spawned.is_err() {
            let _ = self.db.finish_decision(
                &failed_id,
                generation,
                "failed",
                "Could not start participation worker.",
                None,
                None,
            );
            self.signal();
        }
    }
}
pub(crate) fn start(
    chats: Arc<Chats>,
    db: Arc<Database>,
    work: Vec<DecisionWork>,
    root: String,
    _tasks: Vec<Arc<ChatTask>>,
    listener: Arc<dyn ChatListener>,
) -> Option<GroupTask> {
    let first = work.first()?;
    let state = Arc::new(GroupState {
        listener,
        db,
        room: first.decision.conversation_id.clone(),
        message: first.decision.message_id.clone(),
        controls: Mutex::new(Vec::new()),
        tasks: Mutex::new(Vec::new()),
        changed: Mutex::new(0),
        wake: Condvar::new(),
        records: Mutex::new(Vec::new()),
    });
    {
        let mut groups = chats.groups.lock().expect("group controls lock");
        groups.retain(|g| g.strong_count() > 0);
        groups.push(Arc::downgrade(&state));
    }
    chats.wake_groups();
    for work in work.into_iter().filter(|w| w.decision.status == "pending") {
        let persona = work.persona.clone();
        state.classify(work, persona);
    }
    let group = state.clone();
    let queues = chats.clone();
    let spawned = std::thread::Builder::new()
        .name("pebble-room".into())
        .spawn(move || {
            loop {
                let observed = *group.changed.lock().expect("round wake lock");
                group.sync_controls();
                if !group.refresh() {
                    group.cancel();
                }
                match queues.step_group(
                    group.db.clone(),
                    &group.message,
                    std::path::Path::new(&root),
                    &group,
                ) {
                    Ok(Step::Finished) => {
                        group.refresh();
                        group.settled();
                        queues.wake_groups();
                        return;
                    }
                    Ok(Step::Classify(work)) => {
                        for work in work {
                            let persona = work.persona.clone();
                            group.classify(work, persona);
                        }
                        continue;
                    }
                    Ok(Step::Continue) => continue,
                    Ok(Step::Wait) => {}
                    Ok(Step::Admit(..)) => unreachable!("admission launches inside the queue gate"),
                    Err(_) => {
                        group.cancel();
                        group.refresh();
                        group.settled();
                        queues.wake_groups();
                        return;
                    }
                }
                let changed = group.changed.lock().expect("round wake lock");
                drop(
                    group
                        .wake
                        .wait_while(changed, |value| *value == observed)
                        .expect("round wait"),
                );
            }
        });
    if spawned.is_err() {
        state.cancel();
        state.refresh();
        state.settled();
    }
    Some(GroupTask { state })
}
impl GroupState {
    fn sync_controls(&self) {
        for (id, generation, control) in self
            .controls
            .lock()
            .expect("classifier controls lock")
            .iter()
        {
            if !self.db.decision_active(id, *generation) {
                control.cancel();
            }
        }
        for task in self.tasks.lock().expect("group tasks lock").iter() {
            if !self.db.call_active(&task.id) {
                task.cancel();
            }
        }
    }
}
