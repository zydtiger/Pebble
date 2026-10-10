//! Explicit response-contract publication and persona state/context choices.
use crate::{Error, Persona, config, storage::Database};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseSettings {
    pub schema_json: String,
    pub initial_state_json: String,
    pub current_state_json: String,
    pub session_id: Option<String>,
    pub active_schema_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseAction {
    InitialDefaultsOnly,
    FreshContextKeepState,
    FreshContextResetState,
}

#[derive(Debug, Clone)]
pub struct ResponseUpdate {
    pub expected: ResponseSettings,
    pub schema_json: String,
    pub initial_state_json: String,
    pub action: ResponseAction,
}

pub(crate) fn stale() -> Error {
    Error::SettingsSave("State, context, or response defaults changed since this panel loaded. Reload the state controls and review your changes before applying them.".into())
}

fn json(text: &str, label: &str) -> Result<Value, Error> {
    serde_json::from_str(text)
        .map_err(|_| Error::SettingsSave(format!("{label} must contain valid JSON.")))
}

pub(crate) fn save(
    root: &Path,
    db: &Database,
    persona: &Persona,
    update: &ResponseUpdate,
) -> Result<(), Error> {
    db.ensure_idle(Some(&persona.id))?;
    db.ensure_decisions_idle(Some(&persona.id))?;
    let snapshot = db.response_settings(persona)?;
    if snapshot != update.expected {
        return Err(stale());
    }
    let directory = PathBuf::from(&persona.directory);
    let schema_path = directory.join("response_schema.json");
    let initial = json(&update.initial_state_json, "Initial defaults")?;
    let schema = crate::structured::parse(&schema_path, &update.schema_json, &initial)?;
    if update.action == ResponseAction::InitialDefaultsOnly && schema != persona.response_schema {
        return Err(Error::SettingsSave("Changing the response schema requires a fresh context and an explicit keep-state or reset-state choice.".into()));
    }
    if update.action == ResponseAction::FreshContextKeepState {
        let current = json(&snapshot.current_state_json, "Accepted state")?;
        crate::structured::validate_state(&schema, &current).map_err(|detail| Error::SettingsSave(format!("Current accepted state is incompatible with this schema: {detail} Choose reset to the new initial defaults, or revise the schema. No files or state changed.")))?;
    }
    let path = directory.join("config.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| crate::file_error(&path, e))?;
    let mut document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|_| crate::file_error(&path, "Invalid TOML"))?;
    let initial_toml = toml::to_string(&serde_json::json!({"response":{"initial_state":initial}}))
        .map_err(|_| Error::SettingsSave("Initial defaults must be an object representable in TOML; null and out-of-range integers are not supported.".into()))?;
    let initial_document: toml_edit::DocumentMut = initial_toml.parse().map_err(|_| {
        Error::SettingsSave("Initial defaults cannot be represented in TOML.".into())
    })?;
    document["response"]["initial_state"] = initial_document["response"]["initial_state"].clone();
    let mut candidates = Vec::new();
    if schema != persona.response_schema {
        candidates.push((
            schema_path,
            format!("{}\n", serde_json::to_string_pretty(&schema)?),
        ));
    }
    if initial != persona.initial_state {
        candidates.push((path, document.to_string()));
    }
    let mut candidate = config::load_candidate(root, &candidates)?
        .personas
        .into_iter()
        .find(|p| p.id == persona.id)
        .ok_or(Error::UnknownPersona)?;
    candidate.conversation_id = persona.conversation_id.clone();
    db.apply_response_settings(&candidate, update, || {
        crate::settings::files::publish(&candidates)
    })
}
