#[cfg(test)]
extern crate self as pebble_core;

mod chat;
mod codex;
pub use codex::{
    CodexCapabilities, CodexSettings, DelegatedTask, DelegationListener, DelegationState,
    TaskAnswer, TaskChoice, TaskLink, TaskQuestion, TaskRequest,
};
mod classifier_settings;
mod config;
mod connection;
mod context;
mod demo;
mod history;
mod image_generation;
mod images;
pub use image_generation::{
    ChatGptImageSettings, ImageGeneration, ImageGenerationListener, ImageGenerationSettings,
    StudioSettings,
};
pub use images::ImageAsset;
mod live;
mod maintenance;
mod memory;
mod participation;
mod personas;
mod request_parameters;
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
pub use maintenance::{ContextMarker, MaintenanceStatus, MaintenanceTask};
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
    #[error("{0}")]
    Image(String),
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
    codex: Arc<codex::Manager>,
    image_generation: Arc<image_generation::Manager>,
}

impl Core {
    pub fn open(data_root: impl Into<PathBuf>) -> Result<Self, Error> {
        let data_root = data_root.into();
        config::initialize(&data_root)?;
        let database = Arc::new(storage::Database::open(&data_root)?);
        database.upgrade_persona_contracts(&data_root)?;
        config::load(&data_root)?;
        let chats = Arc::new(chat::Chats::default());
        let codex =
            codex::Manager::new(database.clone(), data_root.clone(), Arc::downgrade(&chats));
        *database.codex.lock().expect("task manager") = Arc::downgrade(&codex);
        let image_generation = image_generation::Manager::new(
            database.clone(),
            data_root.clone(),
            Arc::downgrade(&chats),
        );
        *database.image_generation.lock().expect("image manager") =
            Arc::downgrade(&image_generation);
        let core = Self {
            data_root,
            database,
            chats,
            codex,
            image_generation,
        };
        core.workspace()?;
        core.codex.recover();
        core.image_generation.recover();
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
    pub fn add_group_member(&self, room: &str, persona: &str) -> Result<Room, Error> {
        self.chats.serialized(|| {
            if !self.workspace()?.personas.iter().any(|p| p.id == persona) {
                return Err(Error::UnknownPersona);
            }
            self.database.add_group_member(room, persona)
        })
    }
    pub fn remove_group_member(&self, room: &str, persona: &str) -> Result<Room, Error> {
        self.chats
            .remove_group_member(&self.database, room, persona)
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
            chat::RoomInput {
                room,
                text: prompt.trim(),
                mentions,
                images: &[],
                separate_tasks: false,
            },
            listener,
        )
    }

    pub fn import_image(&self, path: &Path) -> Result<ImageAsset, Error> {
        self.database
            .import_image(&images::read_bounded(path, images::MAX_SOURCE_BYTES)?)
    }
    pub fn import_image_data(&self, bytes: &[u8]) -> Result<ImageAsset, Error> {
        self.database.import_image(bytes)
    }
    pub fn send_room_images(
        &self,
        room: &str,
        prompt: &str,
        mentions: &[String],
        images: &[String],
        listener: Arc<dyn ChatListener>,
    ) -> Result<ChatSubmission, Error> {
        if prompt.trim().is_empty() && mentions.is_empty() && images.is_empty() {
            return Err(Error::EmptyMessage);
        }
        self.chats.start_room(
            self.database.clone(),
            || self.workspace(),
            chat::RoomInput {
                room,
                text: prompt.trim(),
                mentions,
                images,
                separate_tasks: false,
            },
            listener,
        )
    }

    pub fn send_room_assigned_images(
        &self,
        room: &str,
        prompt: &str,
        mentions: &[String],
        images: &[String],
        separate_tasks: bool,
        listener: Arc<dyn ChatListener>,
    ) -> Result<ChatSubmission, Error> {
        if prompt.trim().is_empty() && mentions.is_empty() && images.is_empty() {
            return Err(Error::EmptyMessage);
        }
        self.chats.start_room(
            self.database.clone(),
            || self.workspace(),
            chat::RoomInput {
                room,
                text: prompt.trim(),
                mentions,
                images,
                separate_tasks,
            },
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
        let task = self.chats.retry(
            self.database.clone(),
            || self.persona(persona_id),
            reply_id,
            self.codex.retry_listener(listener),
        )?;
        self.codex.notify();
        Ok(task)
    }

    pub fn start_fresh_context(&self, persona_id: &str) -> Result<Arc<MaintenanceTask>, Error> {
        self.chats
            .start_maintenance(self.database.clone(), self.data_root.clone(), || {
                self.persona(persona_id)
            })
    }
    pub fn maintenance_status(&self, persona_id: &str) -> Result<Option<MaintenanceStatus>, Error> {
        self.database.maintenance_status(persona_id)
    }
    pub fn context_markers(&self, room: &str) -> Result<Vec<ContextMarker>, Error> {
        self.database.context_markers(room)
    }
    pub fn fresh_context(&self, persona_id: &str) -> Result<Persona, Error> {
        let task = self.start_fresh_context(persona_id)?;
        loop {
            let status = task.status();
            if status.status != "running" {
                if status.status != "completed" {
                    return Err(Error::Generation(status.detail));
                }
                return self.persona(persona_id);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
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
        self.codex.shutdown();
        self.image_generation.shutdown();
    }
}

impl Core {
    pub fn codex_settings(&self) -> Result<CodexSettings, Error> {
        self.codex.settings()
    }
    pub fn save_codex_settings(&self, settings: &CodexSettings) -> Result<(), Error> {
        self.codex.save_settings(settings)?;
        self.image_generation.invalidate_availability();
        Ok(())
    }
    pub fn codex_capabilities(&self) -> Result<CodexCapabilities, Error> {
        self.codex.capabilities()
    }
    pub fn delegated_tasks(&self) -> Result<Vec<DelegatedTask>, Error> {
        self.database.delegated_tasks()
    }
    pub fn observe_delegations(&self, listener: Arc<dyn DelegationListener>) -> Result<(), Error> {
        self.codex.subscribe(listener)
    }
    pub fn cancel_delegation(&self, id: &str) -> Result<(), Error> {
        self.codex.cancel(id)
    }
    pub fn answer_delegation(
        &self,
        id: &str,
        request: &str,
        approve: Option<bool>,
        answers: &[TaskAnswer],
    ) -> Result<(), Error> {
        self.codex.respond(id, request, approve, answers)
    }
    pub fn followup_delegation(&self, id: &str, prompt: &str) -> Result<(), Error> {
        self.codex.followup(id, prompt)
    }
    pub fn reconcile_delegation(&self, id: &str) -> Result<(), Error> {
        self.codex.reconcile(id)
    }
    pub fn retry_delegation_report(&self, id: &str) -> Result<(), Error> {
        self.codex.retry_report(id)
    }
}

impl Core {
    pub fn image_generation_settings(&self) -> Result<ImageGenerationSettings, Error> {
        image_generation::settings::read(&self.data_root)
    }
    pub fn save_image_generation_settings(
        &self,
        settings: &ImageGenerationSettings,
    ) -> Result<(), Error> {
        self.chats
            .serialized(|| image_generation::settings::save(&self.data_root, settings))?;
        self.image_generation.invalidate_availability();
        Ok(())
    }
    pub fn image_generations(&self) -> Result<Vec<ImageGeneration>, Error> {
        self.database.image_generations()
    }
    pub fn observe_image_generations(
        &self,
        listener: Arc<dyn ImageGenerationListener>,
    ) -> Result<(), Error> {
        self.image_generation.subscribe(listener)
    }
}
