use crate::{BackendError, ListenerError, PebbleBackend};
use std::sync::Arc;
#[derive(Clone, uniffi::Record)]
pub struct StudioSettingsRecord {
    pub base_url: String,
    pub model_id: String,
    pub gpu_id: String,
    pub steps: Option<u32>,
    pub guidance: Option<f64>,
}
#[derive(Clone, uniffi::Record)]
pub struct ChatGptImageSettingsRecord {
    pub model: Option<String>,
    pub effort: Option<String>,
}
#[derive(Clone, uniffi::Record)]
pub struct ImageGenerationSettingsRecord {
    pub studio: Option<StudioSettingsRecord>,
    pub chatgpt: Option<ChatGptImageSettingsRecord>,
}
impl From<pebble_core::ImageGenerationSettings> for ImageGenerationSettingsRecord {
    fn from(s: pebble_core::ImageGenerationSettings) -> Self {
        Self {
            studio: s.studio.map(|s| StudioSettingsRecord {
                base_url: s.base_url,
                model_id: s.model_id,
                gpu_id: s.gpu_id,
                steps: s.steps,
                guidance: s.guidance,
            }),
            chatgpt: s.chatgpt.map(|s| ChatGptImageSettingsRecord {
                model: s.model,
                effort: s.effort,
            }),
        }
    }
}
impl From<ImageGenerationSettingsRecord> for pebble_core::ImageGenerationSettings {
    fn from(s: ImageGenerationSettingsRecord) -> Self {
        Self {
            studio: s.studio.map(|s| pebble_core::StudioSettings {
                base_url: s.base_url,
                model_id: s.model_id,
                gpu_id: s.gpu_id,
                steps: s.steps,
                guidance: s.guidance,
            }),
            chatgpt: s.chatgpt.map(|s| pebble_core::ChatGptImageSettings {
                model: s.model,
                effort: s.effort,
            }),
        }
    }
}
#[derive(Clone, Copy, uniffi::Enum)]
pub enum ImageOperationKind {
    Generate,
    Edit,
}
impl From<pebble_core::ImageOperationKind> for ImageOperationKind {
    fn from(kind: pebble_core::ImageOperationKind) -> Self {
        match kind {
            pebble_core::ImageOperationKind::Generate => Self::Generate,
            pebble_core::ImageOperationKind::Edit => Self::Edit,
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct ImageGenerationRecord {
    pub id: String,
    pub persona_id: String,
    pub conversation_id: String,
    pub kind: ImageOperationKind,
    pub backend: String,
    pub status: String,
    pub activity: String,
    pub detail: String,
    pub publication_status: String,
    pub asset_id: Option<String>,
    pub actual_width: Option<u32>,
    pub actual_height: Option<u32>,
}
impl From<pebble_core::ImageGeneration> for ImageGenerationRecord {
    fn from(o: pebble_core::ImageGeneration) -> Self {
        Self {
            id: o.id,
            persona_id: o.persona_id,
            conversation_id: o.conversation_id,
            kind: o.kind.into(),
            backend: o.backend,
            status: o.status,
            activity: o.activity,
            detail: o.detail,
            publication_status: o.publication_status,
            asset_id: o.asset_id,
            actual_width: o.actual_width,
            actual_height: o.actual_height,
        }
    }
}
#[uniffi::export(foreign)]
pub trait ImageGenerationObserver: Send + Sync {
    fn on_images(&self, operations: Vec<ImageGenerationRecord>) -> Result<(), ListenerError>;
}
struct Observer(Arc<dyn ImageGenerationObserver>);
impl pebble_core::ImageGenerationListener for Observer {
    fn on_images(&self, operations: Vec<pebble_core::ImageGeneration>) -> Result<(), String> {
        self.0
            .on_images(operations.into_iter().map(Into::into).collect())
            .map_err(|e| e.to_string())
    }
}
#[uniffi::export]
impl PebbleBackend {
    pub fn image_generation_settings(&self) -> Result<ImageGenerationSettingsRecord, BackendError> {
        Ok(self.core.image_generation_settings()?.into())
    }
    pub fn save_image_generation_settings(
        &self,
        settings: ImageGenerationSettingsRecord,
    ) -> Result<(), BackendError> {
        Ok(self.core.save_image_generation_settings(&settings.into())?)
    }
    pub fn image_generations(&self) -> Result<Vec<ImageGenerationRecord>, BackendError> {
        Ok(self
            .core
            .image_generations()?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub fn observe_image_generations(
        &self,
        observer: Arc<dyn ImageGenerationObserver>,
    ) -> Result<(), BackendError> {
        Ok(self
            .core
            .observe_image_generations(Arc::new(Observer(observer)))?)
    }
}
