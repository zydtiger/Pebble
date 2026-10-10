use crate::Error;
use serde_json::Value;

/// Conservative UTF-8 byte accounting, not provider token usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    pub context_len: u64,
    pub max_output_tokens: u64,
}
impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            context_len: 65_536,
            max_output_tokens: 4096,
        }
    }
}
impl ContextBudget {
    pub(crate) fn check_demo(&self, request: &crate::ModelRequest) -> Result<(), Error> {
        let items = request
            .items
            .iter()
            .map(|item| serde_json::from_slice::<Value>(&item.payload))
            .collect::<Result<Vec<_>, _>>()?;
        self.check_body(&serde_json::json!({"input":items,"schema":request.response_schema}))
    }
    pub(crate) fn check_body(&self, body: &Value) -> Result<(), Error> {
        // Count the final wire JSON, including all envelopes, schema, profile,
        // native reasoning/state and any future tool metadata. Add framing reserve.
        let estimate = (serde_json::to_vec(body)?.len() as u64).saturating_add(128);
        if estimate > self.context_len.saturating_sub(self.max_output_tokens) {
            return Err(Error::ContextLimit(format!(
                "Context is full: conservative input estimate {estimate} UTF-8 bytes, output reserve {}, configured context_len {}. Increase this connection's limits in config.toml or start a fresh context; earlier messages stay saved. This estimate is not exact provider token usage.",
                self.max_output_tokens, self.context_len
            )));
        }
        Ok(())
    }
}
