mod chat;
mod classifier_settings;
mod config;
mod connection;
mod context;
mod demo;
mod history;
mod live;
mod participation;
mod personas;
mod response_settings;
mod rooms;
mod settings;
mod storage;
mod structured;

pub use chat::{ChatEvent, ChatEventKind, ChatListener, ChatSubmission, ChatTask};
pub use classifier_settings::ClassifierSettings;
pub use config::{ChatConnection, ClassifierConnection, Persona, Workspace};
pub use context::ContextBudget;
pub use history::{Message, MessageStatus, ModelItem, ModelRequest};
pub use participation::{GroupTask, Participation};
pub use response_settings::{ResponseAction, ResponseSettings, ResponseUpdate};
pub use rooms::{Room, RoomMember};
pub use settings::{ChatSettings, CredentialChange, PersonaUpdate, Settings, SettingsUpdate};

use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Could not read {path}: {detail}")]
    File { path: String, detail: String },
    #[error("Check {path}: {detail}")]
    Configuration { path: String, detail: String },
    #[error("Could not access Pebble's database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Could not encode or read model history: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Pebble's data is already open in another app instance.")]
    AlreadyOpen,
    #[error("Database version {0} is newer than this app supports.")]
    Schema(u32),
    #[error("Finish or stop pending replies and participation checks first.")]
    Busy,
    #[error("This character is no longer available. Reload the character files.")]
    UnknownPersona,
    #[error("Write something before sending.")]
    EmptyMessage,
    #[error("Could not start the reply: {0}")]
    Generation(String),
    #[error("{0}")]
    ContextLimit(String),
    #[error("{0}")]
    History(String),
    #[error("{0}")]
    SettingsSave(String),
    #[error("{0}")]
    RetryUnavailable(String),
    #[error("{0}")]
    Room(String),
    #[error("{0}")]
    Participation(String),
}

fn file_error(path: &Path, error: impl std::fmt::Display) -> Error {
    Error::File {
        path: path.display().to_string(),
        detail: error.to_string(),
    }
}

pub struct Core {
    data_root: PathBuf,
    database: Arc<storage::Database>,
    chats: Arc<chat::Chats>,
}

impl Core {
    pub fn open(data_root: impl Into<PathBuf>) -> Result<Self, Error> {
        let data_root = data_root.into();
        config::initialize(&data_root)?;
        let database = Arc::new(storage::Database::open(&data_root)?);
        database.upgrade_persona_contracts(&data_root)?;
        config::load(&data_root)?;
        let core = Self {
            data_root,
            database,
            chats: Arc::new(chat::Chats::default()),
        };
        core.workspace()?;
        Ok(core)
    }

    pub fn workspace(&self) -> Result<Workspace, Error> {
        let mut workspace = config::load(&self.data_root)?;
        self.database
            .resolve_conversations(&mut workspace.personas)?;
        workspace.rooms = self.database.rooms()?;
        Ok(workspace)
    }

    pub fn create_group(&self, title: &str, members: &[String]) -> Result<Room, Error> {
        self.chats.serialized(|| {
            let workspace = self.workspace()?;
            if members
                .iter()
                .any(|id| !workspace.personas.iter().any(|p| &p.id == id))
            {
                return Err(Error::UnknownPersona);
            }
            self.database.create_group(title, members)
        })
    }
    pub fn rename_group(&self, room: &str, title: &str) -> Result<Room, Error> {
        self.chats
            .serialized(|| self.database.rename_group(room, title))
    }
    pub fn room_messages(&self, room: &str) -> Result<Vec<Message>, Error> {
        self.database.room(room)?;
        self.database.messages(room)
    }
    pub fn send_room_message(
        &self,
        room: &str,
        prompt: &str,
        mentions: &[String],
        listener: Arc<dyn ChatListener>,
    ) -> Result<ChatSubmission, Error> {
        if prompt.trim().is_empty() && mentions.is_empty() {
            return Err(Error::EmptyMessage);
        }
        self.chats.start_room(
            self.database.clone(),
            || self.workspace(),
            room,
            prompt.trim(),
            mentions,
            listener,
        )
    }

    pub fn create_persona(&self, name: &str) -> Result<Persona, Error> {
        let id = personas::create(&self.data_root, name)?;
        self.workspace()?
            .personas
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(Error::UnknownPersona)
    }

    pub fn settings(&self, persona_id: Option<&str>) -> Result<Settings, Error> {
        settings::read(&self.data_root, persona_id)
    }

    pub fn classifier_settings(
        &self,
        persona_id: Option<&str>,
    ) -> Result<ClassifierSettings, Error> {
        classifier_settings::read(&self.data_root, persona_id)
    }
    pub fn save_classifier_settings(
        &self,
        persona_id: Option<&str>,
        update: &ClassifierSettings,
        credential: &CredentialChange,
    ) -> Result<Workspace, Error> {
        self.chats.settings(&self.database, persona_id, || {
            classifier_settings::save(&self.data_root, persona_id, update, credential)
        })?;
        self.workspace()
    }
    pub fn participation(&self, room: &str) -> Result<Vec<Participation>, Error> {
        self.database.participation(room)
    }
    pub fn stop_room(&self, room: &str) -> Result<(), Error> {
        self.chats.stop_room(&self.database, room)
    }

    fn persona(&self, id: &str) -> Result<Persona, Error> {
        self.workspace()?
            .personas
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(Error::UnknownPersona)
    }

    pub fn response_settings(&self, persona_id: &str) -> Result<ResponseSettings, Error> {
        self.chats
            .serialized(|| self.database.response_settings(&self.persona(persona_id)?))
    }

    pub fn save_response_settings(
        &self,
        persona_id: &str,
        update: &ResponseUpdate,
    ) -> Result<Workspace, Error> {
        self.chats.serialized(|| {
            let persona = self.persona(persona_id)?;
            response_settings::save(&self.data_root, &self.database, &persona, update)?;
            self.workspace()
        })
    }

    pub fn save_settings(
        &self,
        persona_id: Option<&str>,
        update: &SettingsUpdate,
    ) -> Result<Workspace, Error> {
        self.chats.settings(&self.database, persona_id, || {
            settings::save(&self.data_root, persona_id, update)
        })?;
        self.workspace()
    }

    pub fn retry_message(
        &self,
        persona_id: &str,
        reply_id: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<Arc<ChatTask>, Error> {
        self.chats.retry(
            self.database.clone(),
            || self.persona(persona_id),
            reply_id,
            listener,
        )
    }

    pub fn fresh_context(&self, persona_id: &str) -> Result<Persona, Error> {
        self.chats
            .fresh_context(&self.database, || self.persona(persona_id))?;
        self.workspace()?
            .personas
            .into_iter()
            .find(|p| p.id == persona_id)
            .ok_or(Error::UnknownPersona)
    }

    pub fn send_message(
        &self,
        persona_id: &str,
        prompt: &str,
        listener: Arc<dyn ChatListener>,
    ) -> Result<Arc<ChatTask>, Error> {
        if prompt.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        self.chats.start(
            self.database.clone(),
            || self.persona(persona_id),
            prompt.trim(),
            listener,
        )
    }

    pub fn messages(&self, persona_id: &str) -> Result<Vec<Message>, Error> {
        let persona = self
            .workspace()?
            .personas
            .into_iter()
            .find(|persona| persona.id == persona_id)
            .ok_or(Error::UnknownPersona)?;
        self.database.messages(&persona.conversation_id)
    }

    pub fn model_request(&self, call_id: &str) -> Result<ModelRequest, Error> {
        self.database.request(call_id)
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        self.chats.cancel_all();
    }
}
