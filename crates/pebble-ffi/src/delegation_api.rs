use crate::{BackendError, ListenerError, PebbleBackend};
use std::sync::Arc;
#[derive(Clone, uniffi::Record)]
pub struct CodexSettingsRecord {
    pub enabled: bool,
    pub executable: String,
    pub writable: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
}
impl From<pebble_core::CodexSettings> for CodexSettingsRecord {
    fn from(s: pebble_core::CodexSettings) -> Self {
        Self {
            enabled: s.enabled,
            executable: s.executable,
            writable: s.writable,
            model: s.model,
            effort: s.effort,
        }
    }
}
impl From<CodexSettingsRecord> for pebble_core::CodexSettings {
    fn from(s: CodexSettingsRecord) -> Self {
        Self {
            enabled: s.enabled,
            executable: s.executable,
            writable: s.writable,
            model: s.model,
            effort: s.effort,
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CodexCapabilitiesRecord {
    pub authenticated: bool,
    pub models: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub detail: String,
}
#[derive(Clone, uniffi::Enum)]
pub enum DelegationState {
    Queued,
    Running,
    NeedsInput,
    NeedsApproval,
    CancellationRequested,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
    Uncertain,
}
#[derive(Clone, uniffi::Record)]
pub struct TaskLinkRecord {
    pub label: String,
    pub target: String,
}
#[derive(Clone, uniffi::Record)]
pub struct TaskChoiceRecord {
    pub label: String,
    pub value: String,
}
#[derive(Clone, uniffi::Record)]
pub struct TaskQuestionRecord {
    pub required: bool,
    pub id: String,
    pub text: String,
    pub header: String,
    pub description: String,
    pub guidance: Vec<String>,
    pub secret: bool,
    pub options: Vec<String>,
    pub option_descriptions: Vec<String>,
    pub choices: Vec<TaskChoiceRecord>,
    pub multiple: bool,
}
impl From<pebble_core::TaskQuestion> for TaskQuestionRecord {
    fn from(q: pebble_core::TaskQuestion) -> Self {
        Self {
            id: q.id,
            required: q.required,
            text: q.text,
            header: q.header,
            description: q.description,
            guidance: q.guidance,
            secret: q.secret,
            options: q.options,
            option_descriptions: q.option_descriptions,
            choices: q
                .choices
                .into_iter()
                .map(|c| TaskChoiceRecord {
                    label: c.label,
                    value: c.value,
                })
                .collect(),
            multiple: q.multiple,
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct TaskRequestRecord {
    pub id: String,
    pub approval: bool,
    pub summary: String,
    pub questions: Vec<TaskQuestionRecord>,
    pub authorization_url: Option<String>,
}
#[derive(Clone, uniffi::Record)]
pub struct DelegatedTaskRecord {
    pub id: String,
    pub title: String,
    pub persona_id: String,
    pub conversation_id: String,
    pub state: DelegationState,
    pub activity: String,
    pub detail: String,
    pub result: String,
    pub links: Vec<TaskLinkRecord>,
    pub request: Option<TaskRequestRecord>,
    pub report_state: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub created_at: String,
    pub private_delivery: bool,
}
impl From<pebble_core::DelegatedTask> for DelegatedTaskRecord {
    fn from(t: pebble_core::DelegatedTask) -> Self {
        let private_delivery = t.private_delivery();
        let state = match t.state {
            pebble_core::DelegationState::Queued => DelegationState::Queued,
            pebble_core::DelegationState::Running => DelegationState::Running,
            pebble_core::DelegationState::NeedsInput => DelegationState::NeedsInput,
            pebble_core::DelegationState::NeedsApproval => DelegationState::NeedsApproval,
            pebble_core::DelegationState::CancellationRequested => {
                DelegationState::CancellationRequested
            }
            pebble_core::DelegationState::Completed => DelegationState::Completed,
            pebble_core::DelegationState::Failed => DelegationState::Failed,
            pebble_core::DelegationState::Cancelled => DelegationState::Cancelled,
            pebble_core::DelegationState::Interrupted => DelegationState::Interrupted,
            pebble_core::DelegationState::Uncertain => DelegationState::Uncertain,
        };
        Self {
            id: t.id,
            title: t.title,
            persona_id: t.persona_id,
            conversation_id: t.conversation_id,
            state,
            activity: t.activity,
            detail: t.detail,
            result: t.result,
            links: t
                .links
                .into_iter()
                .map(|l| TaskLinkRecord {
                    label: l.label,
                    target: l.target,
                })
                .collect(),
            request: t.request.map(|r| TaskRequestRecord {
                id: r.id,
                approval: r.approval,
                summary: r.summary,
                authorization_url: r.authorization_url,
                questions: r.questions.into_iter().map(Into::into).collect(),
            }),
            report_state: t.report_state,
            thread_id: t.thread_id,
            turn_id: t.turn_id,
            created_at: t.created_at,
            private_delivery,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn question_decision_context_reaches_typed_native_record() {
        let question = pebble_core::TaskQuestion {
            id: "scope".into(),
            required: true,
            text: "Choose scope".into(),
            header: "Impact".into(),
            description: "Review before answering".into(),
            guidance: vec!["Minimum: 18".into()],
            secret: true,
            options: vec!["A".into(), "B".into()],
            option_descriptions: vec!["Read only".into(), "Write selected files".into()],
            choices: vec![],
            multiple: false,
        };
        let record: TaskQuestionRecord = question.into();
        assert_eq!(record.header, "Impact");
        assert_eq!(record.description, "Review before answering");
        assert_eq!(record.guidance, ["Minimum: 18"]);
        assert_eq!(record.options, ["A", "B"]);
        assert_eq!(
            record.option_descriptions,
            ["Read only", "Write selected files"]
        );
        assert!(record.secret && record.required);
    }
}
#[derive(Clone, uniffi::Record)]
pub struct TaskAnswerRecord {
    pub question_id: String,
    pub answers: Vec<String>,
}
#[uniffi::export(foreign)]
pub trait DelegationObserver: Send + Sync {
    fn on_tasks(&self, tasks: Vec<DelegatedTaskRecord>) -> Result<(), ListenerError>;
}
struct Observer(Arc<dyn DelegationObserver>);
impl pebble_core::DelegationListener for Observer {
    fn on_tasks(&self, tasks: Vec<pebble_core::DelegatedTask>) -> Result<(), String> {
        self.0
            .on_tasks(tasks.into_iter().map(Into::into).collect())
            .map_err(|e| e.to_string())
    }
}
#[uniffi::export]
impl PebbleBackend {
    pub fn codex_settings(&self) -> Result<CodexSettingsRecord, BackendError> {
        Ok(self.core.codex_settings()?.into())
    }
    pub fn save_codex_settings(&self, settings: CodexSettingsRecord) -> Result<(), BackendError> {
        Ok(self.core.save_codex_settings(&settings.into())?)
    }
    pub fn codex_capabilities(&self) -> Result<CodexCapabilitiesRecord, BackendError> {
        let c = self.core.codex_capabilities()?;
        Ok(CodexCapabilitiesRecord {
            authenticated: c.authenticated,
            models: c.models,
            skills: c.skills,
            mcp_servers: c.mcp_servers,
            detail: c.detail,
        })
    }
    pub fn delegated_tasks(&self) -> Result<Vec<DelegatedTaskRecord>, BackendError> {
        Ok(self
            .core
            .delegated_tasks()?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub fn observe_delegations(
        &self,
        observer: Arc<dyn DelegationObserver>,
    ) -> Result<(), BackendError> {
        Ok(self
            .core
            .observe_delegations(Arc::new(Observer(observer)))?)
    }
    pub fn cancel_delegation(&self, id: String) -> Result<(), BackendError> {
        Ok(self.core.cancel_delegation(&id)?)
    }
    pub fn answer_delegation(
        &self,
        id: String,
        request: String,
        approve: Option<bool>,
        answers: Vec<TaskAnswerRecord>,
    ) -> Result<(), BackendError> {
        Ok(self.core.answer_delegation(
            &id,
            &request,
            approve,
            &answers
                .into_iter()
                .map(|a| pebble_core::TaskAnswer {
                    question_id: a.question_id,
                    answers: a.answers,
                })
                .collect::<Vec<_>>(),
        )?)
    }
    pub fn followup_delegation(&self, id: String, prompt: String) -> Result<(), BackendError> {
        Ok(self.core.followup_delegation(&id, &prompt)?)
    }
    pub fn reconcile_delegation(&self, id: String) -> Result<(), BackendError> {
        Ok(self.core.reconcile_delegation(&id)?)
    }
    pub fn retry_delegation_report(&self, id: String) -> Result<(), BackendError> {
        Ok(self.core.retry_delegation_report(&id)?)
    }
}
