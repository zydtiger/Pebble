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
    pub fn maintenance_headroom(&self) -> u64 {
        98_304.min(self.context_len / 3)
    }
    pub(crate) fn check_chat_start(&self, body: &Value) -> Result<(), Error> {
        let estimate = (serde_json::to_vec(body)?.len() as u64).saturating_add(128);
        let ceiling = self
            .context_len
            .saturating_sub(self.max_output_tokens)
            .saturating_sub(self.maintenance_headroom());
        if estimate > ceiling {
            return Err(Error::ContextLimit(format!(
                "Chat context is full: conservative input estimate {estimate} UTF-8 bytes exceeds the ordinary-chat ceiling {ceiling}, which reserves {} bytes for explicit memory maintenance within configured context_len {}. Start fresh context to consolidate memory, or increase this connection's limits. No automatic rollover occurs.",
                self.maintenance_headroom(),
                self.context_len
            )));
        }
        Ok(())
    }
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

#[cfg(test)]
mod memory_budget_tests {
    use super::*;
    #[test]
    fn chat_ceiling_and_maintenance_ceiling_are_exact_and_distinct() {
        let budget = ContextBudget {
            context_len: 262144,
            max_output_tokens: 4096,
        };
        let ceiling = 262144 - 4096 - budget.maintenance_headroom();
        // JSON string quotes and 128 framing bytes count exactly.
        let body = Value::String("x".repeat((ceiling - 130) as usize));
        assert!(budget.check_chat_start(&body).is_ok());
        let over = Value::String("x".repeat((ceiling - 129) as usize));
        assert!(budget.check_chat_start(&over).is_err());
        assert!(budget.check_body(&over).is_ok());
        let full = Value::String("x".repeat((262144 - 4096 - 130) as usize));
        assert!(budget.check_body(&full).is_ok());
        let over = Value::String("x".repeat((262144 - 4096 - 129) as usize));
        assert!(budget.check_body(&over).is_err());
    }
}
