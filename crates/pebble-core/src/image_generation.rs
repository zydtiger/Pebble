//! Background image execution and original-room publication.
mod chatgpt;
mod manager;
mod prompt;
pub(crate) mod settings;
mod studio;
pub(crate) mod tool;
pub(crate) use manager::Manager;
use serde::{Deserialize, Serialize};
pub use settings::{ChatGptImageSettings, ImageGenerationSettings, StudioSettings};

pub(crate) fn failure(detail: &str) -> crate::Error {
    crate::Error::Image(format!("Image generation: {detail}"))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageGeneration {
    pub id: String,
    pub persona_id: String,
    pub conversation_id: String,
    pub backend: String,
    pub status: String,
    pub activity: String,
    pub detail: String,
    pub publication_status: String,
    pub asset_id: Option<String>,
    pub actual_width: Option<u32>,
    pub actual_height: Option<u32>,
    #[serde(default)]
    pub effective_parameters: serde_json::Value,
    pub(crate) arguments: tool::Arguments,
    pub(crate) settings: ImageGenerationSettings,
    pub(crate) executable: String,
    pub(crate) trigger: String,
    pub(crate) membership: Option<String>,
    pub(crate) dispatch_started: bool,
    pub(crate) remote_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) item_id: Option<String>,
    pub(crate) artifact_id: Option<String>,
    pub(crate) report_call: Option<String>,
    pub(crate) reports: Vec<ImageReport>,
    pub(crate) invalidated: bool,
}
impl ImageGeneration {
    pub(crate) fn active(&self) -> bool {
        ["queued", "loading", "running", "cancelling"].contains(&self.status.as_str())
    }
}
pub trait ImageGenerationListener: Send + Sync {
    fn on_images(&self, operations: Vec<ImageGeneration>) -> Result<(), String>;
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod studio_tests;

#[cfg(test)]
mod chatgpt_tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ImageReport {
    pub call: String,
    pub input: serde_json::Value,
}
