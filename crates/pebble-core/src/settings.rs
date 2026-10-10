pub(crate) mod files;
use crate::{Error, config};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, value};

#[derive(Debug, Clone, Default)]
pub struct ChatSettings {
    pub enabled: bool,
    pub model: String,
    pub base_url: String,
    pub api: String,
    pub responses_schema_format: String,
    pub has_api_key: bool,
    pub api_key_env: String,
}
#[derive(Debug, Clone)]
pub struct Settings {
    pub chat: ChatSettings,
    pub context_window_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub persona: Option<PersonaUpdate>,
}
#[derive(Clone)]
pub enum CredentialChange {
    Keep,
    Replace(String),
    Clear,
    Environment(String),
}
impl std::fmt::Debug for CredentialChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Keep => "Keep",
            Self::Replace(_) => "Replace(<redacted>)",
            Self::Clear => "Clear",
            Self::Environment(_) => "Environment",
        })
    }
}
#[derive(Debug, Clone)]
pub struct PersonaUpdate {
    pub display_name: String,
    pub subtitle: String,
    pub soul: String,
    pub memory: String,
}
#[derive(Debug, Clone)]
pub struct SettingsUpdate {
    pub chat: ChatSettings,
    pub credential: CredentialChange,
    pub context_window_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub persona: Option<PersonaUpdate>,
}
pub(crate) fn path(root: &Path, persona: Option<&str>) -> Result<PathBuf, Error> {
    if let Some(id) = persona {
        if !config::load(root)?.personas.iter().any(|p| p.id == id) {
            return Err(Error::UnknownPersona);
        }
        Ok(root.join("personas").join(id).join("config.toml"))
    } else {
        Ok(root.join("config.toml"))
    }
}
pub(crate) fn document(path: &Path) -> Result<DocumentMut, Error> {
    std::fs::read_to_string(path)
        .map_err(|e| crate::file_error(path, e))?
        .parse()
        .map_err(|_| Error::Configuration {
            path: path.display().to_string(),
            detail: "Invalid TOML; open the configuration file to repair it.".into(),
        })
}
pub(crate) fn read(root: &Path, persona: Option<&str>) -> Result<Settings, Error> {
    let path = path(root, persona)?;
    let doc = document(&path)?;
    let empty = Item::None;
    let table = if persona.is_some() {
        doc.as_item()
    } else {
        doc.get("defaults").unwrap_or(&empty)
    };
    let chat = table.get("chat").unwrap_or(&empty);
    let string = |key: &str| {
        chat.get(key)
            .and_then(Item::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let persona = persona
        .map(|id| {
            config::load(root)?
                .personas
                .into_iter()
                .find(|p| p.id == id)
                .ok_or(Error::UnknownPersona)
        })
        .transpose()?
        .map(|p| PersonaUpdate {
            display_name: p.display_name,
            subtitle: p.subtitle,
            soul: p.soul,
            memory: p.memory,
        });
    let number = |key: &str| {
        table
            .get("context")
            .and_then(|t| t.get(key))
            .and_then(Item::as_integer)
            .map(|v| v as u64)
    };
    Ok(Settings {
        chat: ChatSettings {
            enabled: chat.is_table_like(),
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
        },
        context_window_tokens: number("context_window_tokens"),
        max_output_tokens: number("max_output_tokens"),
        persona,
    })
}
pub(crate) fn put(table: &mut Item, key: &str, text: &str) {
    let decor = table
        .get(key)
        .and_then(Item::as_value)
        .map(|v| v.decor().clone());
    table[key] = value(text);
    if let Some(decor) = decor {
        *table[key].as_value_mut().unwrap().decor_mut() = decor;
    }
}
pub(crate) fn save(
    root: &Path,
    persona: Option<&str>,
    update: &SettingsUpdate,
) -> Result<(), Error> {
    if matches!(&update.credential, CredentialChange::Replace(key) if key.trim().is_empty()) {
        return Err(Error::SettingsSave(
            "Enter a replacement key or choose Clear explicitly.".into(),
        ));
    }
    let path = path(root, persona)?;
    let mut doc = document(&path)?;
    if persona.is_some() != update.persona.is_some() {
        return Err(Error::SettingsSave(
            "Settings scope does not match the character form.".into(),
        ));
    }
    let table = if persona.is_some() {
        doc.as_item_mut()
    } else {
        &mut doc["defaults"]
    };
    if update.chat.enabled {
        for (key, text) in [
            ("model", update.chat.model.as_str()),
            ("base_url", update.chat.base_url.as_str()),
            ("api", update.chat.api.as_str()),
        ] {
            put(&mut table["chat"], key, text);
        }
        let chat = &mut table["chat"];
        if update.chat.api == "responses" {
            put(
                chat,
                "responses_schema_format",
                &update.chat.responses_schema_format,
            );
        } else {
            chat.as_table_like_mut()
                .unwrap()
                .remove("responses_schema_format");
        }
        match &update.credential {
            CredentialChange::Keep => {}
            CredentialChange::Replace(key) => {
                chat.as_table_like_mut().unwrap().remove("api_key_env");
                put(chat, "api_key", key);
            }
            CredentialChange::Clear => {
                chat.as_table_like_mut().unwrap().remove("api_key_env");
                put(chat, "api_key", "");
            }
            CredentialChange::Environment(name) => {
                chat.as_table_like_mut().unwrap().remove("api_key");
                put(chat, "api_key_env", name);
            }
        }
    } else if let Some(table) = table.as_table_like_mut() {
        table.remove("chat");
    }
    for (key, number) in [
        ("context_window_tokens", update.context_window_tokens),
        ("max_output_tokens", update.max_output_tokens),
    ] {
        if let Some(number) = number {
            let number = i64::try_from(number)
                .map_err(|_| Error::SettingsSave("Context limit is too large.".into()))?;
            table["context"][key] = value(number);
        } else if let Some(context) = table.get_mut("context").and_then(Item::as_table_like_mut) {
            context.remove(key);
        }
    }
    let mut candidates = Vec::new();
    if let Some(persona) = &update.persona {
        put(&mut doc["persona"], "display_name", &persona.display_name);
        put(&mut doc["persona"], "subtitle", &persona.subtitle);
        let directory = path.parent().unwrap();
        candidates.push((directory.join("SOUL.md"), persona.soul.clone()));
        candidates.push((directory.join("MEMORY.md"), persona.memory.clone()));
    }
    candidates.push((path, doc.to_string())); // TOML is committed last.
    config::load_candidate(root, &candidates)?; // Validate all inherited dependent budgets before any mutation.
    files::publish(&candidates)
}
