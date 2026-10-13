use super::{ImageGenerationListener, failure};
use crate::{
    Error,
    chat::{Chats, Control},
    storage::Database,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

pub(crate) struct Manager {
    pub(crate) db: Arc<Database>,
    pub(crate) root: PathBuf,
    chats: Weak<Chats>,
    controls: Mutex<HashMap<String, Arc<Control>>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    listeners: Mutex<Vec<Arc<dyn ImageGenerationListener>>>,
    stopped: AtomicBool,
    availability: Mutex<Option<String>>,
}
impl Manager {
    pub(crate) fn new(db: Arc<Database>, root: PathBuf, chats: Weak<Chats>) -> Arc<Self> {
        Arc::new(Self {
            db,
            root,
            chats,
            controls: Mutex::new(HashMap::new()),
            workers: Mutex::new(Vec::new()),
            listeners: Mutex::new(Vec::new()),
            stopped: AtomicBool::new(false),
            availability: Mutex::new(None),
        })
    }
    pub(crate) fn invalidate_availability(&self) {
        *self.availability.lock().expect("image availability") = None;
    }
    pub(crate) fn available_backends(&self) -> Result<Vec<String>, Error> {
        let settings = super::settings::read(&self.root)?;
        let mut backends = Vec::new();
        if settings.studio.is_some() {
            backends.push("studio".into());
        }
        if settings.chatgpt.is_some() {
            let executable = crate::codex::settings::read(&self.root)?.executable;
            let key = serde_json::to_string(&(&settings, &executable))?;
            let mut cached = self.availability.lock().expect("image availability");
            let available =
                cached.as_ref() == Some(&key) || super::chatgpt::available(&self.root, &executable);
            // Login or a transient child failure may change outside Pebble. Cache success only.
            *cached = available.then_some(key);
            if available {
                backends.push("chatgpt".into());
            }
        }
        Ok(backends)
    }
    pub(crate) fn subscribe(
        &self,
        listener: Arc<dyn ImageGenerationListener>,
    ) -> Result<(), Error> {
        listener
            .on_images(self.db.image_generations()?)
            .map_err(|_| failure("Image view disconnected."))?;
        self.listeners
            .lock()
            .expect("image listeners")
            .push(listener);
        Ok(())
    }
    pub(crate) fn notify(&self) {
        if let Ok(ops) = self.db.image_generations() {
            self.listeners
                .lock()
                .expect("image listeners")
                .retain(|l| l.on_images(ops.clone()).is_ok());
        }
    }
    pub(crate) fn recover(self: &Arc<Self>) {
        // Saved assets can publish safely; no external submission resumes on startup.
        if let Ok(ops) = self.db.image_generations() {
            for op in ops {
                if !op.invalidated && op.status == "uncertain" && op.remote_id.is_some() {
                    let _ = self.reconcile(&op.id);
                } else if op.publication_status == "pending" {
                    let _ = self.report(&op.id);
                }
            }
        }
    }
    pub(crate) fn dispatch(self: &Arc<Self>, id: &str) -> Result<(), Error> {
        self.launch(id, false)
    }
    pub(crate) fn reconcile(self: &Arc<Self>, id: &str) -> Result<(), Error> {
        let op = self.db.image_generation(id)?;
        if op.active() || op.invalidated {
            return Err(failure("Finish or stop current image work first."));
        }
        if op.status == "uncertain" && op.remote_id.is_some() {
            self.launch(id, true)
        } else if op.asset_id.is_some() {
            self.db.update_image_generation(id, |o| {
                if o.publication_status != "completed" {
                    o.publication_status = "pending".into();
                }
                Ok(())
            })?;
            self.report(id)
        } else if op.remote_id.is_some() {
            self.launch(id, true)
        } else {
            Err(failure(
                "No remote identity was confirmed. The submission may have succeeded; it will not be repeated. Ask for a new explicit image request.",
            ))
        }
    }
    fn launch(self: &Arc<Self>, id: &str, recover: bool) -> Result<(), Error> {
        let mut controls = self.controls.lock().expect("image controls");
        if controls.contains_key(id) || self.stopped.load(Ordering::Acquire) {
            return Err(failure(
                "Image work is already running or Pebble is closing.",
            ));
        }
        let control = Arc::new(Control::default());
        controls.insert(id.into(), control.clone());
        let manager = self.clone();
        let id = id.to_owned();
        let worker_id = id.clone();
        let worker = std::thread::Builder::new()
            .name("pebble-image".into())
            .spawn(move || {
                let before = manager
                    .db
                    .image_generation(&id)
                    .ok()
                    .map(|o| (o.status, o.detail, o.asset_id));
                let result = manager.execute(&id, recover, &control);
                if let Err(error) = result {
                    let _ = manager.db.update_image_generation(&id, |o| {
                        if o.invalidated {
                            o.status = if o.dispatch_started {
                                "uncertain"
                            } else {
                                "cancelled"
                            }
                            .into();
                        } else {
                            o.status = if o.dispatch_started {
                                "uncertain"
                            } else {
                                "failed"
                            }
                            .into();
                        }
                        o.activity.clear();
                        let previous = std::mem::take(&mut o.detail);
                        o.detail = if o.asset_id.is_some() {
                            format!("{previous} Controller/transport outcome: {error}")
                        } else {
                            error.to_string()
                        };
                        Ok(())
                    });
                }
                if let Ok(op) = manager.db.image_generation(&id) {
                    let after = (op.status.clone(), op.detail.clone(), op.asset_id.clone());
                    if !op.invalidated && !op.active() && before.as_ref() != Some(&after) {
                        let _ = manager.db.update_image_generation(&id, |o| {
                            // Execution evidence may change after the saved pixels were published.
                            if o.publication_status != "completed" {
                                o.publication_status = "pending".into();
                                o.report_call = None;
                            }
                            Ok(())
                        });
                    }
                }
                manager.controls.lock().expect("image controls").remove(&id);
                let _ = manager.report(&id);
                manager.notify();
            })
            .map_err(|_| failure("Could not start image worker."));
        match worker {
            Ok(worker) => self.workers.lock().expect("image workers").push(worker),
            Err(error) => {
                controls.remove(&worker_id);
                return Err(error);
            }
        }
        drop(controls);
        self.notify();
        Ok(())
    }
    fn execute(&self, id: &str, recover: bool, control: &Arc<Control>) -> Result<(), Error> {
        if control.is_cancelled() {
            return Err(failure("Cancelled before image execution."));
        }
        let op = self.db.image_generation(id)?;
        if op.invalidated {
            return Err(failure("Image publication was cancelled."));
        }
        if op.backend == "studio" {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| failure("Could not initialize image worker."))?;
            runtime.block_on(super::studio::execute(self, id, recover, control))
        } else {
            super::chatgpt::execute(self, id, recover, control)
        }
    }
    pub(crate) fn progress(&self, id: &str, status: &str, activity: &str) -> Result<(), Error> {
        self.db.update_image_generation(id, |o| {
            if !o.invalidated {
                o.status = status.into();
                o.activity = activity.into();
            }
            Ok(())
        })?;
        self.notify();
        Ok(())
    }
    pub(crate) fn import(&self, id: &str, bytes: &[u8]) -> Result<(), Error> {
        if self.db.image_generation(id)?.asset_id.is_some() {
            return Ok(());
        }
        let asset = self.db.import_image(bytes)?;
        self.db.update_image_generation(id,|o| {
            o.asset_id=Some(asset.id);o.actual_width=Some(asset.width);o.actual_height=Some(asset.height);
            let mismatch=asset.width!=o.arguments.size.width || asset.height!=o.arguments.size.height;
            o.detail=if mismatch {format!("Requested {} × {}; generated {} × {}. {}",o.arguments.size.width,o.arguments.size.height,asset.width,asset.height,if o.backend=="studio" {"Studio returned an unsupported size; the asset was retained for recovery, but it cannot be published as success."}else{"ChatGPT sizing is best effort; no regeneration was attempted."})}else{format!("Generated {} × {} pixels.",asset.width,asset.height)};
            if mismatch && o.backend=="studio" { o.status="failed".into(); }
            Ok(())
        })?;
        self.notify();
        Ok(())
    }
    pub(crate) fn cancel_ids(&self, ids: &[String]) {
        let controls = self.controls.lock().expect("image controls");
        for id in ids {
            if let Some(c) = controls.get(id) {
                c.cancel();
            }
        }
        drop(controls);
        self.notify();
    }
    fn report(self: &Arc<Self>, id: &str) -> Result<(), Error> {
        if self.stopped.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(chats) = self.chats.upgrade() {
            chats.image_report(
                self.db.clone(),
                &self.root,
                id,
                Arc::new(ReportListener(Arc::downgrade(self))),
            )?;
        }
        self.notify();
        Ok(())
    }
    pub(crate) fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        for c in self.controls.lock().expect("image controls").values() {
            c.cancel();
        }
        let workers = std::mem::take(&mut *self.workers.lock().expect("image workers"));
        for w in workers {
            let _ = w.join();
        }
    }
}
struct ReportListener(Weak<Manager>);
impl crate::ChatListener for ReportListener {
    fn on_event(&self, _: crate::ChatEvent) -> Result<(), String> {
        if let Some(m) = self.0.upgrade() {
            m.notify();
        }
        Ok(())
    }
}
