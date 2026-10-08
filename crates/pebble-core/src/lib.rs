mod config;
mod preview;
mod storage;

pub use config::{ChatConnection, Persona, Workspace};
pub use preview::{PreviewEvent, PreviewEventKind, PreviewListener, PreviewTask};

use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Could not read {path}: {detail}")]
    File { path: String, detail: String },
    #[error("Check {path}: {detail}")]
    Configuration { path: String, detail: String },
    #[error("Could not open Pebble's database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Finish or stop the current preview first.")]
    Busy,
    #[error("This character is no longer available. Reload the character files.")]
    UnknownPersona,
    #[error("Write something before starting a preview.")]
    EmptyMessage,
    #[error("Could not start the preview: {0}")]
    Preview(String),
}

fn file_error(path: &Path, error: impl std::fmt::Display) -> Error {
    Error::File {
        path: path.display().to_string(),
        detail: error.to_string(),
    }
}

pub struct Core {
    data_root: PathBuf,
    previews: preview::Previews,
}

impl Core {
    pub fn open(data_root: impl Into<PathBuf>) -> Result<Self, Error> {
        let data_root = data_root.into();
        config::initialize(&data_root)?;
        let core = Self {
            data_root,
            previews: preview::Previews::default(),
        };
        core.workspace()?;
        Ok(core)
    }

    pub fn workspace(&self) -> Result<Workspace, Error> {
        let mut workspace = config::load(&self.data_root)?;
        storage::resolve_conversations(&self.data_root, &mut workspace.personas)?;
        Ok(workspace)
    }

    pub fn start_preview(
        &self,
        persona_id: &str,
        prompt: &str,
        listener: Arc<dyn PreviewListener>,
    ) -> Result<PreviewTask, Error> {
        if prompt.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let persona = self
            .workspace()?
            .personas
            .into_iter()
            .find(|persona| persona.id == persona_id)
            .ok_or(Error::UnknownPersona)?;
        self.previews.start(persona, prompt, listener)
    }
}
