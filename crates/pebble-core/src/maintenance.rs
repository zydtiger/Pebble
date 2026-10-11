use crate::{Error, Persona, chat::Control, storage::Database};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct MaintenanceStatus {
    pub id: String,
    pub persona_id: String,
    pub status: String,
    pub detail: String,
    pub memory_saved: bool,
    pub memory_reviewed: bool,
}
pub struct MaintenanceTask {
    pub id: String,
    pub(crate) control: Arc<Control>,
    pub(crate) latest: Arc<Mutex<MaintenanceStatus>>,
}
impl MaintenanceTask {
    pub fn cancel(&self) {
        self.control.cancel();
    }
    pub fn status(&self) -> MaintenanceStatus {
        self.latest.lock().expect("maintenance status").clone()
    }
}
impl Drop for MaintenanceTask {
    fn drop(&mut self) {
        self.cancel();
    }
}
#[derive(Debug, Clone)]
pub struct ContextMarker {
    pub id: String,
    pub persona_id: String,
    pub label: String,
    pub after_sequence: i64,
    pub created_at: String,
}

pub(crate) fn worker(
    db: Arc<Database>,
    root: PathBuf,
    persona: Persona,
    job: crate::storage::maintenance::Job,
    control: Arc<Control>,
    latest: Arc<Mutex<MaintenanceStatus>>,
) {
    let result = (|| {
        if control.is_cancelled() {
            return Err(Error::Generation(
                "Cancelled before consolidation; the old context remains active.".into(),
            ));
        }
        if job.has_knowledge
            && !db.memory_reviewed(&job.id)?
            && let Some(chat) = &persona.chat
        {
            let request = job.request.as_ref().expect("frozen maintenance request");
            // Unlike chat, maintenance may use the reserved headroom. Never truncate.
            persona.context.check_body(
                &crate::live::request_body(chat, request, persona.context.max_output_tokens)?.1,
            )?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| {
                    Error::Generation("Could not initialize memory maintenance.".into())
                })?;
            let output=runtime.block_on(async {tokio::select!{biased; _=control.cancelled()=>Err(Error::Generation("Cancelled before memory effects.".into())), result=tokio::time::timeout(std::time::Duration::from_secs(180),crate::live::generate(chat,request,persona.context.max_output_tokens,control.clone(),|_|Ok(())))=>result.map_err(|_|Error::Generation("Memory maintenance exceeded 180 seconds.".into()))?}})?;
            db.archive_maintenance(&job.id, &output)?;
            let calls = crate::memory::calls(&output)?;
            let call=calls.first().ok_or_else(||Error::Generation("The chat model did not call update_memory. The old context remains active; retry memory maintenance.".into()))?;
            let result = db.execute_memory(
                crate::storage::memory::MemoryContext {
                    owner: &job.id,
                    persona: &persona,
                    session: None,
                    protocol: &request.protocol,
                    expected: &job.memory,
                },
                &output,
                call,
                &control,
            )?;
            db.archive_maintenance_result(&job.id, &result)?;
            crate::memory::check_result(&result)?;
        }
        let loaded = crate::config::load(&root)?
            .personas
            .into_iter()
            .find(|p| p.id == persona.id)
            .ok_or(Error::UnknownPersona)?;
        control
            .while_active(|| db.publish_maintenance(&job, &loaded))
            .unwrap_or_else(|| {
                Err(Error::Generation(
                    "Cancelled; the old context remains active.".into(),
                ))
            })
    })();
    let saved = db.memory_saved(&job.id).unwrap_or(false);
    let reviewed = db.memory_reviewed(&job.id).unwrap_or(false);
    let (status, detail) = match result {
        Ok(()) => (
            "completed",
            if persona.chat.is_none() {
                "Fresh local demo context; no chat model is connected, so MEMORY was kept unchanged.".into()
            } else if !job.has_knowledge {
                "Fresh empty context; no outgoing knowledge needed consolidation.".into()
            } else {
                "Memory reviewed and fresh context started. Accepted state and transcripts were preserved.".into()
            },
        ),
        Err(e) => (
            if control.is_cancelled() {
                "cancelled"
            } else {
                "failed"
            },
            format!(
                "{}{}",
                match e {
                    Error::Generation(detail) => detail,
                    other => other.to_string(),
                },
                if saved {
                    " Memory was saved, but the old context remains active. Retry Start fresh context to publish the saved result without repeating the write."
                } else if reviewed {
                    " Memory was reviewed and unchanged, but the old context remains active. Retry Start fresh context to publish the completed review without another model request."
                } else {
                    " The old context remains active; no success marker was added."
                }
            ),
        ),
    };
    let _ = db.finish_maintenance(&job.id, status, &detail);
    *latest.lock().expect("maintenance status") = MaintenanceStatus {
        id: job.id,
        persona_id: persona.id,
        status: status.into(),
        detail,
        memory_saved: saved,
        memory_reviewed: reviewed,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_after_identical_review_retains_completed_review_status() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::Core::open(root.path()).unwrap();
        let persona = core.workspace().unwrap().personas.remove(0);
        let job = core.database.begin_maintenance(&persona).unwrap();
        let id = job.id.clone();
        let call = crate::memory::Call {
            id: "unchanged-review".into(),
            arguments: serde_json::json!({"content":job.memory}).to_string(),
            content: job.memory.clone(),
        };
        core.database
            .execute_memory(
                crate::storage::memory::MemoryContext {
                    owner: &id,
                    persona: &persona,
                    session: None,
                    protocol: &job.binding.0,
                    expected: &job.memory,
                },
                &[],
                &call,
                &Control::default(),
            )
            .unwrap();
        let latest = Arc::new(Mutex::new(
            core.database
                .maintenance_status(&persona.id)
                .unwrap()
                .unwrap(),
        ));
        let control = Arc::new(Control::default());
        control.cancel();
        worker(
            core.database.clone(),
            root.path().to_path_buf(),
            persona.clone(),
            job,
            control,
            latest.clone(),
        );
        let status = latest.lock().unwrap().clone();
        assert_eq!(status.status, "cancelled");
        assert!(status.memory_reviewed);
        assert!(!status.memory_saved);
        assert!(status.detail.contains("reviewed and unchanged"));
        assert!(status.detail.contains("without another model request"));
        assert!(
            core.context_markers(&persona.conversation_id)
                .unwrap()
                .is_empty()
        );
        drop(core);
        let core = crate::Core::open(root.path()).unwrap();
        let restored = core.maintenance_status(&persona.id).unwrap().unwrap();
        assert!(restored.memory_reviewed);
        assert!(!restored.memory_saved);
        assert_eq!(restored.detail, status.detail);
    }
}
