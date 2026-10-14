use crate::*;

#[derive(Clone, uniffi::Record)]
pub struct ParticipationRecord {
    pub id: String,
    pub message_id: String,
    pub conversation_id: String,
    pub persona_id: String,
    pub membership_id: String,
    pub status: String,
    pub slot_status: String,
    pub phase: String,
    pub error: String,
    pub call_id: Option<String>,
    pub elapsed_ms: Option<u64>,
    pub revision: i64,
    pub respond: Option<bool>,
}
impl From<pebble_core::Participation> for ParticipationRecord {
    fn from(p: pebble_core::Participation) -> Self {
        Self {
            id: p.id,
            message_id: p.message_id,
            conversation_id: p.conversation_id,
            persona_id: p.persona_id,
            membership_id: p.membership_id,
            status: p.status,
            slot_status: p.slot_status,
            phase: p.phase,
            error: p.error,
            call_id: p.call_id,
            elapsed_ms: p.elapsed_ms,
            revision: p.revision,
            respond: p.respond,
        }
    }
}
#[derive(uniffi::Object)]
pub struct GroupHandle {
    pub(crate) task: pebble_core::GroupTask,
}
#[uniffi::export]
impl GroupHandle {
    pub fn id(&self) -> String {
        self.task.id().to_owned()
    }
    pub fn handles(&self) -> Vec<Arc<ChatHandle>> {
        self.task
            .tasks()
            .into_iter()
            .map(|task| Arc::new(ChatHandle { task }))
            .collect()
    }
    pub fn cancel(&self) {
        self.task.cancel();
    }
    pub fn cancel_decision(&self, id: String) {
        self.task.cancel_decision(&id);
    }
}
#[derive(uniffi::Record)]
pub struct ClassifierSettingsRecord {
    pub connection: ConnectionSettingsRecord,
}
impl From<pebble_core::ClassifierSettings> for ClassifierSettingsRecord {
    fn from(s: pebble_core::ClassifierSettings) -> Self {
        Self {
            connection: s.connection.into(),
        }
    }
}
impl From<ConnectionSettingsRecord> for pebble_core::ChatSettings {
    fn from(s: ConnectionSettingsRecord) -> Self {
        Self {
            enabled: s.enabled,
            model: s.model,
            base_url: s.base_url,
            api: s.api,
            responses_schema_format: s.responses_schema_format,
            request_json: s.request_json,
            has_api_key: s.has_api_key,
            api_key_env: s.api_key_env,
            context_len: s.context_len,
            max_output_tokens: s.max_output_tokens,
        }
    }
}
impl From<pebble_core::ChatSettings> for ConnectionSettingsRecord {
    fn from(s: pebble_core::ChatSettings) -> Self {
        Self {
            enabled: s.enabled,
            model: s.model,
            base_url: s.base_url,
            api: s.api,
            responses_schema_format: s.responses_schema_format,
            request_json: s.request_json,
            has_api_key: s.has_api_key,
            api_key_env: s.api_key_env,
            context_len: s.context_len,
            max_output_tokens: s.max_output_tokens,
        }
    }
}
impl From<CredentialUpdate> for pebble_core::CredentialChange {
    fn from(s: CredentialUpdate) -> Self {
        match s {
            CredentialUpdate::Keep => Self::Keep,
            CredentialUpdate::Clear => Self::Clear,
            CredentialUpdate::Replace { value } => Self::Replace(value),
            CredentialUpdate::Environment { name } => Self::Environment(name),
        }
    }
}
#[uniffi::export]
impl PebbleBackend {
    pub fn participation(&self, room_id: String) -> Result<Vec<ParticipationRecord>, BackendError> {
        Ok(self
            .core
            .participation(&room_id)?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub fn classifier_settings(
        &self,
        persona_id: Option<String>,
    ) -> Result<ClassifierSettingsRecord, BackendError> {
        Ok(self.core.classifier_settings(persona_id.as_deref())?.into())
    }
    pub fn save_classifier_settings(
        &self,
        persona_id: Option<String>,
        update: ClassifierSettingsRecord,
        credential: CredentialUpdate,
    ) -> Result<WorkspaceRecord, BackendError> {
        Ok(self
            .core
            .save_classifier_settings(
                persona_id.as_deref(),
                &pebble_core::ClassifierSettings {
                    connection: update.connection.into(),
                },
                &credential.into(),
            )?
            .into())
    }
}
