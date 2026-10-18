pub(crate) mod adapter;
mod manager;
pub(crate) mod settings;
pub(crate) mod tool;
use crate::Error;
pub use adapter::CodexCapabilities;
pub(crate) use manager::Manager;
use serde::{Deserialize, Serialize};
pub use settings::CodexSettings;
pub(crate) use settings::TaskSettings;

pub(crate) fn failure(detail: &str) -> Error {
    Error::Generation(format!("Codex: {detail}"))
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
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
impl DelegationState {
    pub fn active(self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Running
                | Self::NeedsInput
                | Self::NeedsApproval
                | Self::CancellationRequested
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLink {
    pub label: String,
    pub target: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChoice {
    pub label: String,
    pub value: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskQuestion {
    #[serde(default = "required_question")]
    pub required: bool,
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub guidance: Vec<String>,
    pub secret: bool,
    pub options: Vec<String>,
    #[serde(default)]
    pub option_descriptions: Vec<String>,
    #[serde(default)]
    pub choices: Vec<TaskChoice>,
    #[serde(default)]
    pub multiple: bool,
}
fn required_question() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TaskReport {
    pub call: String,
    pub revision: u64,
    pub input: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    pub id: String,
    pub approval: bool,
    pub summary: String,
    pub questions: Vec<TaskQuestion>,
    #[serde(default)]
    pub authorization_url: Option<String>,
    #[serde(default)]
    pub(crate) method: String,
    #[serde(default)]
    pub(crate) payload: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatedTask {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub(crate) summary_captured: bool,
    #[serde(default)]
    pub(crate) interaction_deferred: bool,
    pub persona_id: String,
    pub conversation_id: String,
    pub state: DelegationState,
    pub activity: String,
    pub detail: String,
    #[serde(default)]
    pub(crate) local_limitation: String,
    pub result: String,
    pub links: Vec<TaskLink>,
    pub request: Option<TaskRequest>,
    #[serde(default)]
    pub(crate) pending_requests: Vec<TaskRequest>,
    #[serde(default)]
    pub(crate) resolved_requests: Vec<String>,
    #[serde(default)]
    pub(crate) authorization_url_digests: Vec<AuthorizationUrlIdentity>,
    pub report_state: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub created_at: String,
    pub(crate) settings: settings::TaskSettings,
    pub(crate) trigger: String,
    pub(crate) membership: Option<String>,
    pub(crate) instructions: String,
    #[serde(default)]
    pub(crate) original_request: String,
    #[serde(default)]
    pub(crate) followup_request: String,
    pub(crate) request_key: String,
    pub(crate) generation: u64,
    pub(crate) report_call: Option<String>,
    #[serde(default)]
    pub(crate) outcome_revision: u64,
    #[serde(default)]
    pub(crate) reports: Vec<TaskReport>,
    pub(crate) submission: String,
    #[serde(default)]
    pub(crate) dispatch_started: bool,
    #[serde(default)]
    pub(crate) approval_previews: std::collections::BTreeMap<String, String>,
    #[serde(default = "user_task_origin")]
    pub(crate) origin: String,
    #[serde(default = "room_report_delivery")]
    pub(crate) delivery: String,
    #[serde(default)]
    pub(crate) activity_id: Option<String>,
    #[serde(default)]
    pub(crate) activity_step_id: Option<String>,
}
fn user_task_origin() -> String {
    "user".into()
}
fn room_report_delivery() -> String {
    "room_report".into()
}
impl DelegatedTask {
    pub fn private_delivery(&self) -> bool {
        self.delivery == "private_observation"
    }
    pub(crate) fn awaiting_first_dispatch(&self) -> bool {
        matches!(
            self.state,
            DelegationState::Queued | DelegationState::Running
        ) && !self.dispatch_started
            && self.turn_id.is_none()
            && self.request.is_none()
            && self.pending_requests.is_empty()
    }
}
pub trait DelegationListener: Send + Sync {
    fn on_tasks(&self, tasks: Vec<DelegatedTask>) -> Result<(), String>;
}
#[derive(Debug, Clone)]
pub struct TaskAnswer {
    pub question_id: String,
    pub answers: Vec<String>,
}

#[cfg(test)]
mod probes;

/// Runtime local date/offset for relative calendar and current-fact requests.
pub(crate) fn local_time() -> serde_json::Value {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let raw = seconds as libc::time_t;
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    let mut buffer = [0i8; 96];
    let formatted = unsafe {
        if libc::localtime_r(&raw, local.as_mut_ptr()).is_null() {
            String::new()
        } else {
            let format = c"%Y-%m-%d %H:%M:%S %Z (%z)";
            libc::strftime(
                buffer.as_mut_ptr(),
                buffer.len(),
                format.as_ptr(),
                local.as_ptr(),
            );
            std::ffi::CStr::from_ptr(buffer.as_ptr())
                .to_string_lossy()
                .into_owned()
        }
    };
    serde_json::json!({"unix_seconds":seconds,"local_date_time_and_offset":formatted})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AuthorizationUrlIdentity {
    pub(crate) digest: String,
    pub(crate) bytes: usize,
}
