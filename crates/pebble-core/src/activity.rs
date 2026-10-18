//! Durable, message-free persona activity scheduling.
//!
//! This layer deliberately keeps timers and runtime ownership outside Swift. A
//! wake is only an opportunity for a private evaluation; it never creates a
//! visible message, changes a room timestamp, or sends a notification.
pub(crate) mod settings;
use crate::{Error, Persona, chat::Chats, storage::Database};
use std::sync::{
    Arc, Condvar, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivitySettings {
    pub enabled: bool,
    pub quiet_start_minutes: Option<u16>,
    pub quiet_end_minutes: Option<u16>,
    pub daily_wake_budget: u8,
    pub next_wake_at: Option<String>,
    pub next_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaActivitySnapshot {
    pub activity_id: Option<String>,
    pub persona_id: String,
    pub objective: String,
    pub summary: String,
    pub phase: String,
    pub active_task_ids: Vec<String>,
    pub next_check_at: Option<String>,
    pub commitment_summary: String,
    pub last_publication_id: Option<String>,
    pub enabled: bool,
}

impl From<crate::storage::activity::ActivitySettingsRow> for ActivitySettings {
    fn from(value: crate::storage::activity::ActivitySettingsRow) -> Self {
        Self {
            enabled: value.enabled,
            quiet_start_minutes: value.quiet_start_minutes,
            quiet_end_minutes: value.quiet_end_minutes,
            daily_wake_budget: value.daily_wake_budget,
            next_wake_at: value.next_wake_at,
            next_reason: value.next_reason,
        }
    }
}
impl From<crate::storage::activity::ActivitySnapshotRow> for PersonaActivitySnapshot {
    fn from(value: crate::storage::activity::ActivitySnapshotRow) -> Self {
        Self {
            activity_id: value.activity_id,
            persona_id: value.persona_id,
            objective: value.objective,
            summary: value.summary,
            phase: value.phase,
            active_task_ids: value.active_task_ids,
            next_check_at: value.next_check_at,
            commitment_summary: value.commitment_summary,
            last_publication_id: value.last_publication_id,
            enabled: value.enabled,
        }
    }
}

pub(crate) struct Manager {
    database: Arc<Database>,
    chats: Weak<Chats>,
    stopped: AtomicBool,
    wake: Arc<(Mutex<bool>, Condvar)>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Manager {
    pub(crate) fn new(database: Arc<Database>, chats: Weak<Chats>) -> Arc<Self> {
        Arc::new(Self {
            database,
            chats,
            stopped: AtomicBool::new(false),
            wake: Arc::new((Mutex::new(false), Condvar::new())),
            worker: Mutex::new(None),
        })
    }

    pub(crate) fn recover(self: &Arc<Self>) {
        let manager = Arc::downgrade(self);
        let wake = self.wake.clone();
        // Pebble has no background service. The loop owns no user data and ends
        // with Core; its only responsibility is due-time coalescing while open.
        let worker = std::thread::Builder::new()
            .name("pebble-activity-scheduler".into())
            .spawn(move || {
                loop {
                    let Some(manager) = manager.upgrade() else {
                        return;
                    };
                    if manager.stopped.load(Ordering::Acquire) {
                        return;
                    }
                    let _ = manager.tick();
                    // Do not retain the database lock file while the app is gone.
                    drop(manager);
                    let (stop, signal) = &*wake;
                    let stop = stop.lock().expect("scheduler wake");
                    let (stop, _) = signal
                        .wait_timeout_while(stop, std::time::Duration::from_secs(1), |stop| !*stop)
                        .expect("scheduler wake");
                    if *stop {
                        return;
                    }
                }
            });
        *self.worker.lock().expect("scheduler worker") = worker.ok();
    }

    pub(crate) fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        let (stop, signal) = &*self.wake;
        *stop.lock().expect("scheduler wake") = true;
        signal.notify_all();
        if let Some(worker) = self.worker.lock().expect("scheduler worker").take() {
            let _ = worker.join();
        }
    }

    fn tick(&self) -> Result<(), Error> {
        let personas = crate::config::load(&self.database.root_path())?.personas;
        for persona in personas {
            if !settings::read(&self.database.root_path(), &persona.id)?.active()
                && let Some(chats) = self.chats.upgrade()
            {
                chats.cancel_private(&persona.id);
            }
            if let Some(step) = self.database.claim_due_activity(&persona.id)? {
                // The real private-call worker is queued through Chats. Keep the
                // scheduler's admission and the worker's terminal update apart:
                // a user chat may cancel an unaccepted private call between them.
                crate::activity::worker::enqueue(
                    self.database.clone(),
                    self.chats.clone(),
                    persona,
                    step,
                )?;
            }
        }
        Ok(())
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
    }
}

pub(crate) mod worker {
    use super::*;

    /// The scheduler has already persisted the wake. Private execution is
    /// purpose-specific and has no room/message listener. Until a configured
    /// model session accepts an activity contract, an enabled local-demo persona
    /// records a bounded quiet evaluation and waits; it never fabricates a reply.
    pub(crate) fn enqueue(
        database: Arc<Database>,
        chats: Weak<Chats>,
        persona: Persona,
        step: String,
    ) -> Result<(), Error> {
        let chats = chats
            .upgrade()
            .ok_or_else(|| Error::Generation("Pebble is closing.".into()))?;
        chats.start_private_activity(database, persona, step)
    }
}
