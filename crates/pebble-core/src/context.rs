use crate::{Error, ModelRequest};
use serde::Deserialize;

/// A deliberately conservative estimate, not provider token accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    pub context_window_tokens: u64,
    pub max_output_tokens: u64,
}
impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            context_window_tokens: 65_536,
            max_output_tokens: 4096,
        }
    }
}
#[derive(Default, Deserialize)]
pub(crate) struct Overrides {
    context_window_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
}
impl Overrides {
    pub(crate) fn apply(
        &self,
        inherited: ContextBudget,
        path: &std::path::Path,
    ) -> Result<ContextBudget, Error> {
        let budget = ContextBudget {
            context_window_tokens: self
                .context_window_tokens
                .unwrap_or(inherited.context_window_tokens),
            max_output_tokens: self
                .max_output_tokens
                .unwrap_or(inherited.max_output_tokens),
        };
        if budget.max_output_tokens == 0 || budget.max_output_tokens >= budget.context_window_tokens
        {
            return Err(Error::Configuration { path: path.display().to_string(), detail: "context limits must be positive and max_output_tokens must be smaller than context_window_tokens".into() });
        }
        Ok(budget)
    }
}
impl ContextBudget {
    pub(crate) fn check(&self, request: &ModelRequest) -> Result<(), Error> {
        let estimate = request
            .items
            .iter()
            .try_fold(0u64, |total, item| {
                total
                    .checked_add(item.payload.len() as u64)?
                    .checked_add(64)
            })
            .ok_or_else(|| {
                Error::ContextLimit(
                    "The context estimate overflowed. Start a fresh context.".into(),
                )
            })?;
        if estimate > self.context_window_tokens - self.max_output_tokens {
            return Err(Error::ContextLimit(format!(
                "Context is full: conservative input estimate {estimate}, output reserve {}, configured window {}. Increase [context] limits in config.toml or start a fresh context; earlier messages stay saved. This estimate is not exact provider token usage.",
                self.max_output_tokens, self.context_window_tokens
            )));
        }
        Ok(())
    }
}
