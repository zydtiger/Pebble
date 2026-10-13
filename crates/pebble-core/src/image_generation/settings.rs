use super::failure;
use crate::{Error, file_error};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImageGenerationSettings {
    pub studio: Option<StudioSettings>,
    pub chatgpt: Option<ChatGptImageSettings>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudioSettings {
    pub base_url: String,
    pub model_id: String,
    pub gpu_id: String,
    pub steps: Option<u32>,
    pub guidance: Option<f64>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChatGptImageSettings {
    pub model: Option<String>,
    pub effort: Option<String>,
}
impl ImageGenerationSettings {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if let Some(s) = &self.studio {
            let url = url::Url::parse(&s.base_url)
                .map_err(|_| failure("Studio requires an HTTP(S) base URL."))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(failure(
                    "Studio URL must have a host and no credentials, query or fragment.",
                ));
            }
            if s.model_id.trim().is_empty() || s.gpu_id.trim().is_empty() {
                return Err(failure("Select a Studio model and GPU."));
            }
            if s.steps.is_some_and(|v| v == 0 || v > 1000)
                || s.guidance
                    .is_some_and(|v| !v.is_finite() || !(0.0..=100.0).contains(&v))
            {
                return Err(failure("Studio inference overrides are invalid."));
            }
        }
        if let Some(s) = &self.chatgpt {
            let c = crate::CodexSettings {
                model: s.model.clone(),
                effort: s.effort.clone(),
                ..Default::default()
            };
            c.validate()?;
        }
        Ok(())
    }
}
pub(crate) fn read(root: &Path) -> Result<ImageGenerationSettings, Error> {
    #[derive(Deserialize)]
    struct Config {
        #[serde(default)]
        image_generation: ImageGenerationSettings,
    }
    let path = root.join("config.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| file_error(&path, e))?;
    let config: Config =
        toml::from_str(&text).map_err(|_| failure("Invalid image_generation configuration."))?;
    config.image_generation.validate()?;
    Ok(config.image_generation)
}
pub(crate) fn save(root: &Path, settings: &ImageGenerationSettings) -> Result<(), Error> {
    settings.validate()?;
    let path = root.join("config.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| file_error(&path, e))?;
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| failure("Invalid app configuration."))?;
    let table = toml::to_string(settings)
        .map_err(|_| failure("Invalid image settings."))?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| failure("Invalid image settings."))?;
    doc["image_generation"] = toml_edit::Item::Table(table.as_table().clone());
    crate::settings::files::publish(&[(path, doc.to_string())])
}
