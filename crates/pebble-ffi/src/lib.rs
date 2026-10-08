use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BackendError {
    #[error("Check {path}: {detail}")]
    Configuration { path: String, detail: String },
    #[error("{detail}")]
    Storage { detail: String },
    #[error("Finish or stop the current preview first.")]
    Busy,
    #[error("This character is no longer available. Reload the character files.")]
    UnknownPersona,
    #[error("Write something before starting a preview.")]
    EmptyMessage,
    #[error("Could not start the preview: {detail}")]
    Preview { detail: String },
}

impl From<pebble_core::Error> for BackendError {
    fn from(error: pebble_core::Error) -> Self {
        match error {
            pebble_core::Error::Configuration { path, detail } => {
                Self::Configuration { path, detail }
            }
            pebble_core::Error::Busy => Self::Busy,
            pebble_core::Error::UnknownPersona => Self::UnknownPersona,
            pebble_core::Error::EmptyMessage => Self::EmptyMessage,
            pebble_core::Error::Preview(detail) => Self::Preview { detail },
            error => Self::Storage {
                detail: error.to_string(),
            },
        }
    }
}

#[derive(Clone, uniffi::Record)]
pub struct PersonaRecord {
    pub id: String,
    pub conversation_id: String,
    pub display_name: String,
    pub subtitle: String,
    pub directory: String,
    pub soul: String,
    pub memory: String,
    pub model: Option<String>,
    pub api: Option<String>,
}

#[derive(uniffi::Record)]
pub struct WorkspaceRecord {
    pub data_root: String,
    pub personas: Vec<PersonaRecord>,
}

impl From<pebble_core::Workspace> for WorkspaceRecord {
    fn from(workspace: pebble_core::Workspace) -> Self {
        Self {
            data_root: workspace.data_root,
            personas: workspace
                .personas
                .into_iter()
                .map(|persona| PersonaRecord {
                    id: persona.id,
                    conversation_id: persona.conversation_id,
                    display_name: persona.display_name,
                    subtitle: persona.subtitle,
                    directory: persona.directory,
                    soul: persona.soul,
                    memory: persona.memory,
                    model: persona.chat.as_ref().map(|chat| chat.model.clone()),
                    api: persona.chat.map(|chat| chat.api),
                })
                .collect(),
        }
    }
}

#[derive(uniffi::Enum)]
pub enum PreviewKind {
    Started,
    TextDelta,
    Completed,
    Cancelled,
}

#[derive(uniffi::Record)]
pub struct PreviewUpdate {
    pub run_id: String,
    pub conversation_id: String,
    pub persona_id: String,
    pub sequence: u64,
    pub kind: PreviewKind,
    pub text: String,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ListenerError {
    #[error("The view stopped listening: {detail}")]
    Closed { detail: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for ListenerError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Closed {
            detail: error.to_string(),
        }
    }
}

#[uniffi::export(foreign)]
pub trait PreviewObserver: Send + Sync {
    fn on_update(&self, update: PreviewUpdate) -> Result<(), ListenerError>;
}

struct ObserverAdapter(Arc<dyn PreviewObserver>);

impl pebble_core::PreviewListener for ObserverAdapter {
    fn on_event(&self, event: pebble_core::PreviewEvent) -> Result<(), String> {
        let kind = match event.kind {
            pebble_core::PreviewEventKind::Started => PreviewKind::Started,
            pebble_core::PreviewEventKind::TextDelta => PreviewKind::TextDelta,
            pebble_core::PreviewEventKind::Completed => PreviewKind::Completed,
            pebble_core::PreviewEventKind::Cancelled => PreviewKind::Cancelled,
        };
        self.0
            .on_update(PreviewUpdate {
                run_id: event.run_id,
                conversation_id: event.conversation_id,
                persona_id: event.persona_id,
                sequence: event.sequence,
                kind,
                text: event.text,
            })
            .map_err(|error| error.to_string())
    }
}

#[derive(uniffi::Object)]
pub struct PreviewHandle {
    task: pebble_core::PreviewTask,
}

#[uniffi::export]
impl PreviewHandle {
    pub fn id(&self) -> String {
        self.task.id.clone()
    }

    pub fn cancel(&self) {
        self.task.cancel();
    }
}

#[derive(uniffi::Object)]
pub struct PebbleBackend {
    core: pebble_core::Core,
}

#[uniffi::export]
impl PebbleBackend {
    #[uniffi::constructor]
    pub fn new(data_root: String) -> Result<Arc<Self>, BackendError> {
        Ok(Arc::new(Self {
            core: pebble_core::Core::open(data_root)?,
        }))
    }

    pub fn workspace(&self) -> Result<WorkspaceRecord, BackendError> {
        Ok(self.core.workspace()?.into())
    }

    pub fn start_preview(
        &self,
        persona_id: String,
        prompt: String,
        observer: Arc<dyn PreviewObserver>,
    ) -> Result<Arc<PreviewHandle>, BackendError> {
        let task =
            self.core
                .start_preview(&persona_id, &prompt, Arc::new(ObserverAdapter(observer)))?;
        Ok(Arc::new(PreviewHandle { task }))
    }
}
