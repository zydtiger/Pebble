use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BackendError {
    #[error("Check {path}: {detail}")]
    Configuration { path: String, detail: String },
    #[error("{detail}")]
    Storage { detail: String },
    #[error("Finish or stop the current reply first.")]
    Busy,
    #[error("This character is no longer available. Reload the character files.")]
    UnknownPersona,
    #[error("Write something before sending.")]
    EmptyMessage,
    #[error("Could not start the reply: {detail}")]
    Generation { detail: String },
    #[error("{detail}")]
    ContextLimit { detail: String },
}

impl From<pebble_core::Error> for BackendError {
    fn from(error: pebble_core::Error) -> Self {
        match error {
            pebble_core::Error::Configuration { path, detail } => {
                Self::Configuration { path, detail }
            }
            pebble_core::Error::Busy => Self::Busy,
            pebble_core::Error::ContextLimit(detail) => Self::ContextLimit { detail },
            pebble_core::Error::UnknownPersona => Self::UnknownPersona,
            pebble_core::Error::EmptyMessage => Self::EmptyMessage,
            pebble_core::Error::Generation(detail) => Self::Generation { detail },
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
    pub connection_label: String,
    pub context_notice: String,
    pub context_window_tokens: u64,
    pub max_output_tokens: u64,
}

#[derive(uniffi::Record)]
pub struct WorkspaceRecord {
    pub data_root: String,
    pub personas: Vec<PersonaRecord>,
}

impl From<pebble_core::Persona> for PersonaRecord {
    fn from(persona: pebble_core::Persona) -> Self {
        Self {
            id: persona.id,
            conversation_id: persona.conversation_id,
            display_name: persona.display_name,
            subtitle: persona.subtitle,
            directory: persona.directory,
            soul: persona.soul,
            memory: persona.memory,
            connection_label: persona.connection_label,
            context_notice: persona.context_notice,
            context_window_tokens: persona.context.context_window_tokens,
            max_output_tokens: persona.context.max_output_tokens,
            model: persona.chat.as_ref().map(|chat| chat.model.clone()),
            api: persona.chat.map(|chat| chat.api),
        }
    }
}
impl From<pebble_core::Workspace> for WorkspaceRecord {
    fn from(workspace: pebble_core::Workspace) -> Self {
        Self {
            data_root: workspace.data_root,
            personas: workspace.personas.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(uniffi::Enum)]
pub enum ChatKind {
    Started,
    TextDelta,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, uniffi::Enum)]
pub enum MessageStatus {
    Complete,
    Streaming,
    Queued,
    Cancelled,
    Interrupted,
    Failed,
}

#[derive(Clone, uniffi::Record)]
pub struct MessageRecord {
    pub id: String,
    pub conversation_id: String,
    pub sequence: i64,
    pub sender_kind: String,
    pub sender_id: String,
    pub content: String,
    pub status: MessageStatus,
    pub created_at: String,
}

impl From<pebble_core::Message> for MessageRecord {
    fn from(message: pebble_core::Message) -> Self {
        let status = match message.status {
            pebble_core::MessageStatus::Complete => MessageStatus::Complete,
            pebble_core::MessageStatus::Streaming => MessageStatus::Streaming,
            pebble_core::MessageStatus::Queued => MessageStatus::Queued,
            pebble_core::MessageStatus::Cancelled => MessageStatus::Cancelled,
            pebble_core::MessageStatus::Interrupted => MessageStatus::Interrupted,
            pebble_core::MessageStatus::Failed => MessageStatus::Failed,
        };
        Self {
            id: message.id,
            conversation_id: message.conversation_id,
            sequence: message.sequence,
            sender_kind: message.sender_kind,
            sender_id: message.sender_id,
            content: message.content,
            status,
            created_at: message.created_at,
        }
    }
}

#[derive(uniffi::Record)]
pub struct ChatUpdate {
    pub run_id: String,
    pub conversation_id: String,
    pub persona_id: String,
    pub sequence: u64,
    pub kind: ChatKind,
    pub text: String,
    pub message_id: String,
    pub messages: Vec<MessageRecord>,
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
pub trait ChatObserver: Send + Sync {
    fn on_update(&self, update: ChatUpdate) -> Result<(), ListenerError>;
}

struct ObserverAdapter(Arc<dyn ChatObserver>);

impl pebble_core::ChatListener for ObserverAdapter {
    fn on_event(&self, event: pebble_core::ChatEvent) -> Result<(), String> {
        let kind = match event.kind {
            pebble_core::ChatEventKind::Started => ChatKind::Started,
            pebble_core::ChatEventKind::TextDelta => ChatKind::TextDelta,
            pebble_core::ChatEventKind::Completed => ChatKind::Completed,
            pebble_core::ChatEventKind::Cancelled => ChatKind::Cancelled,
            pebble_core::ChatEventKind::Failed => ChatKind::Failed,
        };
        self.0
            .on_update(ChatUpdate {
                run_id: event.run_id,
                conversation_id: event.conversation_id,
                persona_id: event.persona_id,
                sequence: event.sequence,
                kind,
                text: event.text,
                message_id: event.message_id,
                messages: event.messages.into_iter().map(Into::into).collect(),
            })
            .map_err(|error| error.to_string())
    }
}

#[derive(uniffi::Object)]
pub struct ChatHandle {
    task: pebble_core::ChatTask,
}

#[uniffi::export]
impl ChatHandle {
    pub fn id(&self) -> String {
        self.task.id.clone()
    }

    pub fn cancel(&self) {
        self.task.cancel();
    }

    pub fn messages(&self) -> Vec<MessageRecord> {
        self.task
            .messages
            .clone()
            .into_iter()
            .map(Into::into)
            .collect()
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

    pub fn create_persona(&self, name: String) -> Result<PersonaRecord, BackendError> {
        Ok(self.core.create_persona(&name)?.into())
    }

    pub fn fresh_context(&self, persona_id: String) -> Result<PersonaRecord, BackendError> {
        Ok(self.core.fresh_context(&persona_id)?.into())
    }

    pub fn messages(&self, persona_id: String) -> Result<Vec<MessageRecord>, BackendError> {
        Ok(self
            .core
            .messages(&persona_id)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub fn send_message(
        &self,
        persona_id: String,
        prompt: String,
        observer: Arc<dyn ChatObserver>,
    ) -> Result<Arc<ChatHandle>, BackendError> {
        let task =
            self.core
                .send_message(&persona_id, &prompt, Arc::new(ObserverAdapter(observer)))?;
        Ok(Arc::new(ChatHandle { task }))
    }
}
