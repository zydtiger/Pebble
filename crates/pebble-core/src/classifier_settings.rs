use crate::{ChatSettings, CredentialChange, Error, config, settings};
use std::path::Path;
use toml_edit::Item;

#[derive(Debug, Clone)]
pub struct ClassifierSettings {
    pub connection: ChatSettings,
    /// Empty means standard protocol; the Qwen extension is explicit and Chat-only.
    pub request_profile: String,
}

pub(crate) fn read(root: &Path, persona: Option<&str>) -> Result<ClassifierSettings, Error> {
    let workspace = config::load(root)?;
    let inherited = persona
        .and_then(|id| workspace.personas.iter().find(|p| p.id == id))
        .and_then(|p| p.classifier.as_ref())
        .map(|c| &c.connection);
    let path = settings::path(root, persona)?;
    let doc = settings::document(&path)?;
    let empty = Item::None;
    let table = if persona.is_some() {
        doc.as_item()
    } else {
        doc.get("defaults").unwrap_or(&empty)
    };
    let classifier = table.get("classifier").unwrap_or(&empty);
    let string = |key: &str| {
        classifier
            .get(key)
            .and_then(Item::as_str)
            .unwrap_or("")
            .to_owned()
    };
    Ok(ClassifierSettings {
        connection: ChatSettings {
            enabled: classifier.is_table_like(),
            model: string("model"),
            base_url: string("base_url"),
            api: if string("api").is_empty() {
                "chat_completions".into()
            } else {
                string("api")
            },
            responses_schema_format: if string("responses_schema_format").is_empty() {
                "text_format".into()
            } else {
                string("responses_schema_format")
            },
            has_api_key: !string("api_key").trim().is_empty(),
            api_key_env: string("api_key_env"),
            context_len: classifier
                .get("context_len")
                .and_then(Item::as_integer)
                .map(|v| v as u64)
                .or_else(|| inherited.map(|c| c.context_len)),
            max_output_tokens: classifier
                .get("max_output_tokens")
                .and_then(Item::as_integer)
                .map(|v| v as u64)
                .or_else(|| inherited.map(|c| c.max_output_tokens)),
        },
        request_profile: string("request_profile"),
    })
}

pub(crate) fn save(
    root: &Path,
    persona: Option<&str>,
    update: &ClassifierSettings,
    credential: &CredentialChange,
) -> Result<(), Error> {
    if matches!(credential, CredentialChange::Replace(key) if key.trim().is_empty()) {
        return Err(Error::SettingsSave(
            "Enter a replacement key or choose Clear explicitly.".into(),
        ));
    }
    let path = settings::path(root, persona)?;
    let mut doc = settings::document(&path)?;
    let table = if persona.is_some() {
        doc.as_item_mut()
    } else {
        &mut doc["defaults"]
    };
    if update.connection.enabled {
        let classifier = &mut table["classifier"];
        for (key, text) in [
            ("model", update.connection.model.as_str()),
            ("base_url", &update.connection.base_url),
            ("api", &update.connection.api),
        ] {
            settings::put(classifier, key, text);
        }
        if update.connection.api == "responses" {
            settings::put(
                classifier,
                "responses_schema_format",
                &update.connection.responses_schema_format,
            );
            classifier
                .as_table_like_mut()
                .unwrap()
                .remove("request_profile");
        } else {
            classifier
                .as_table_like_mut()
                .unwrap()
                .remove("responses_schema_format");
            if update.request_profile.is_empty() {
                classifier
                    .as_table_like_mut()
                    .unwrap()
                    .remove("request_profile");
            } else {
                settings::put(classifier, "request_profile", &update.request_profile);
            }
        }
        settings::put_limits(classifier, &update.connection)?;
        match credential {
            CredentialChange::Keep => {}
            CredentialChange::Replace(key) => {
                classifier
                    .as_table_like_mut()
                    .unwrap()
                    .remove("api_key_env");
                settings::put(classifier, "api_key", key);
            }
            CredentialChange::Clear => {
                classifier
                    .as_table_like_mut()
                    .unwrap()
                    .remove("api_key_env");
                settings::put(classifier, "api_key", "");
            }
            CredentialChange::Environment(name) => {
                classifier.as_table_like_mut().unwrap().remove("api_key");
                settings::put(classifier, "api_key_env", name);
            }
        }
    } else if let Some(table) = table.as_table_like_mut() {
        table.remove("classifier");
    }
    let candidates = [(path, doc.to_string())];
    config::load_candidate(root, &candidates)?;
    settings::files::publish(&candidates)
}
