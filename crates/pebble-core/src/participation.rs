//! Independent participation decisions, never model history or persona state.
mod http;
mod input;
pub(crate) mod worker;
pub(crate) use input::{clip, compact_input};
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

#[derive(Debug, Clone)]
pub struct Participation {
    pub id: String,
    pub message_id: String,
    pub conversation_id: String,
    pub persona_id: String,
    pub membership_id: String,
    pub status: String,
    pub error: String,
    pub call_id: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub revision: i64,
    pub respond: Option<bool>,
    pub(crate) event_cutoff: i64,
}

pub(crate) struct DecisionWork {
    pub decision: Participation,
    pub input: String,
}
