//! Independent participation decisions, never model history or persona state.
mod http;
pub(crate) use http::classify;
mod input;
pub(crate) mod worker;
pub(crate) use input::clip;
pub(crate) use input::round_input;
use serde::Deserialize;
pub use worker::GroupTask;
pub(crate) use worker::start;

pub(crate) const PROMPT: &str = include_str!("../assets/participation-prompt.txt");
pub(crate) fn schema() -> serde_json::Value {
    serde_json::json!({"type":"object","properties":{"respond":{"type":"boolean"}},"required":["respond"],"additionalProperties":false})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    respond: bool,
}
pub(crate) fn answer(text: &str) -> Result<bool, crate::Error> {
    serde_json::from_str::<Answer>(text)
        .map(|a| a.respond)
        .map_err(|_| {
            crate::Error::Participation(
                "Participation model returned an invalid decision. Check its settings or mention this character directly.".into(),
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participation {
    pub id: String,
    pub message_id: String,
    pub conversation_id: String,
    pub persona_id: String,
    pub membership_id: String,
    pub status: String,
    pub slot_status: String,
    pub phase: String,
    pub error: String,
    pub call_id: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub revision: i64,
    pub respond: Option<bool>,
    pub(crate) event_cutoff: i64,
    pub(crate) trigger_message_id: String,
}

pub(crate) struct DecisionWork {
    pub decision: Participation,
    pub input: String,
    pub generation: i64,
    pub phase: String,
    pub persona: crate::Persona,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    whole_group_question: bool,
}
pub(crate) fn phase_answer(text: &str, phase: &str) -> Result<bool, crate::Error> {
    if phase == "intent" {
        serde_json::from_str::<Intent>(text).map(|v|v.whole_group_question).map_err(|_|crate::Error::Participation("Participation model returned an invalid decision. Mention a character or check participation settings.".into()))
    } else {
        answer(text)
    }
}
