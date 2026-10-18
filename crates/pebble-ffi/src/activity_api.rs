use crate::{BackendError, PebbleBackend};

#[derive(Clone, uniffi::Record)]
pub struct ActivitySettingsRecord {
    pub enabled: bool,
    pub quiet_start_minutes: Option<u16>,
    pub quiet_end_minutes: Option<u16>,
    pub daily_wake_budget: u8,
    pub next_wake_at: Option<String>,
    pub next_reason: String,
}
impl From<pebble_core::ActivitySettings> for ActivitySettingsRecord {
    fn from(value: pebble_core::ActivitySettings) -> Self {
        Self {
            enabled: value.enabled,
            quiet_start_minutes: value.quiet_start_minutes,
            quiet_end_minutes: value.quiet_end_minutes,
            daily_wake_budget: value.daily_wake_budget,
            next_wake_at: value.next_wake_at,
            next_reason: value.next_reason,
        }
    }
}
impl From<ActivitySettingsRecord> for pebble_core::ActivitySettings {
    fn from(value: ActivitySettingsRecord) -> Self {
        Self {
            enabled: value.enabled,
            quiet_start_minutes: value.quiet_start_minutes,
            quiet_end_minutes: value.quiet_end_minutes,
            daily_wake_budget: value.daily_wake_budget,
            next_wake_at: value.next_wake_at,
            next_reason: value.next_reason,
        }
    }
}

#[derive(Clone, uniffi::Record)]
pub struct PersonaActivitySnapshotRecord {
    pub activity_id: Option<String>,
    pub persona_id: String,
    pub objective: String,
    pub summary: String,
    pub phase: String,
    pub active_task_ids: Vec<String>,
    pub next_check_at: Option<String>,
    pub commitment_summary: String,
    pub last_publication_id: Option<String>,
    pub enabled: bool,
}
impl From<pebble_core::PersonaActivitySnapshot> for PersonaActivitySnapshotRecord {
    fn from(value: pebble_core::PersonaActivitySnapshot) -> Self {
        Self {
            activity_id: value.activity_id,
            persona_id: value.persona_id,
            objective: value.objective,
            summary: value.summary,
            phase: value.phase,
            active_task_ids: value.active_task_ids,
            next_check_at: value.next_check_at,
            commitment_summary: value.commitment_summary,
            last_publication_id: value.last_publication_id,
            enabled: value.enabled,
        }
    }
}

#[uniffi::export]
impl PebbleBackend {
    pub fn activity_settings(
        &self,
        persona_id: String,
    ) -> Result<ActivitySettingsRecord, BackendError> {
        Ok(self.core.activity_settings(&persona_id)?.into())
    }
    pub fn save_activity_settings(
        &self,
        persona_id: String,
        settings: ActivitySettingsRecord,
    ) -> Result<(), BackendError> {
        Ok(self
            .core
            .save_activity_settings(&persona_id, &settings.into())?)
    }
    pub fn persona_activity(
        &self,
        persona_id: String,
    ) -> Result<PersonaActivitySnapshotRecord, BackendError> {
        Ok(self.core.persona_activity(&persona_id)?.into())
    }
    pub fn persona_activities(&self) -> Result<Vec<PersonaActivitySnapshotRecord>, BackendError> {
        Ok(self
            .core
            .persona_activities()?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub fn start_activity(
        &self,
        persona_id: String,
        objective: String,
    ) -> Result<(), BackendError> {
        Ok(self.core.start_activity(&persona_id, &objective)?)
    }
    pub fn stop_activity(&self, persona_id: String) -> Result<(), BackendError> {
        Ok(self.core.stop_activity(&persona_id)?)
    }
}
