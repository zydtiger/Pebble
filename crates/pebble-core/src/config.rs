use crate::{Error, file_error};
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Workspace {
    pub data_root: String,
    pub personas: Vec<Persona>,
}

#[derive(Debug, Clone)]
pub struct Persona {
    pub id: String,
    pub conversation_id: String,
    pub display_name: String,
    pub subtitle: String,
    pub directory: String,
    pub soul: String,
    pub memory: String,
    pub chat: Option<ChatConnection>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatConnection {
    pub model: String,
    pub base_url: String,
    pub api: String,
    pub api_key_env: Option<String>,
}

#[derive(Deserialize, Default)]
struct Defaults {
    chat: Option<ChatConnection>,
}

#[derive(Deserialize)]
struct AppConfig {
    #[serde(default)]
    defaults: Defaults,
}

#[derive(Deserialize)]
struct PersonaConfig {
    persona: Identity,
    chat: Option<ChatConnection>,
}

#[derive(Deserialize)]
struct Identity {
    display_name: String,
    #[serde(default)]
    subtitle: String,
}

fn read(path: &Path) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|error| file_error(path, error))
}

fn parse<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Error> {
    toml::from_str(&read(path)?).map_err(|error| Error::Configuration {
        path: path.display().to_string(),
        detail: error.to_string(),
    })
}

fn validate_connection(connection: &ChatConnection, path: &Path) -> Result<(), Error> {
    let invalid = |detail: &str| Error::Configuration {
        path: path.display().to_string(),
        detail: detail.into(),
    };
    if connection.model.trim().is_empty() {
        return Err(invalid("chat.model must not be empty"));
    }
    let url = url::Url::parse(&connection.base_url)
        .map_err(|_| invalid("chat.base_url must be a complete http:// or https:// URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid(
            "chat.base_url must use http:// or https:// with a host",
        ));
    }
    if !matches!(connection.api.as_str(), "chat_completions" | "responses") {
        return Err(invalid("chat.api must be chat_completions or responses"));
    }
    if connection.api_key_env.as_ref().is_some_and(|name| {
        name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }) {
        return Err(invalid(
            "chat.api_key_env must name an environment variable",
        ));
    }
    Ok(())
}

pub(crate) fn initialize(root: &Path) -> Result<(), Error> {
    // Only seed an empty directory. Existing user files are never overwritten.
    let empty = if root.exists() {
        fs::read_dir(root)
            .map_err(|error| file_error(root, error))?
            .next()
            .is_none()
    } else {
        true
    };
    if !empty {
        return Ok(());
    }
    let persona = root.join("personas").join(Uuid::new_v4().to_string());
    fs::create_dir_all(&persona).map_err(|error| file_error(&persona, error))?;
    let files = [
        (
            root.join("config.toml"),
            include_str!("../assets/config.toml"),
        ),
        (
            persona.join("config.toml"),
            include_str!("../assets/persona.toml"),
        ),
        (persona.join("SOUL.md"), include_str!("../assets/SOUL.md")),
        (
            persona.join("MEMORY.md"),
            include_str!("../assets/MEMORY.md"),
        ),
    ];
    for (path, contents) in files {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| file_error(&path, error))?;
        file.write_all(contents.as_bytes())
            .map_err(|error| file_error(&path, error))?;
    }
    Ok(())
}

pub(crate) fn load(root: &Path) -> Result<Workspace, Error> {
    let app_path = root.join("config.toml");
    let app: AppConfig = parse(&app_path)?;
    if let Some(connection) = &app.defaults.chat {
        validate_connection(connection, &app_path)?;
    }
    let personas_path = root.join("personas");
    let directories =
        fs::read_dir(&personas_path).map_err(|error| file_error(&personas_path, error))?;
    let mut personas = Vec::new();
    for entry in directories {
        let entry = entry.map_err(|error| file_error(&personas_path, error))?;
        if !entry
            .file_type()
            .map_err(|error| file_error(&entry.path(), error))?
            .is_dir()
        {
            continue;
        }
        let path = entry.path();
        let id = entry.file_name().to_string_lossy().into_owned();
        if Uuid::parse_str(&id).is_err() {
            return Err(Error::Configuration {
                path: path.display().to_string(),
                detail:
                    "Character folder names must be UUIDs; edit display_name to rename a character"
                        .into(),
            });
        }
        let config_path = path.join("config.toml");
        let config: PersonaConfig = parse(&config_path)?;
        if config.persona.display_name.trim().is_empty() {
            return Err(Error::Configuration {
                path: config_path.display().to_string(),
                detail: "persona.display_name must not be empty".into(),
            });
        }
        if let Some(connection) = &config.chat {
            validate_connection(connection, &config_path)?;
        }
        personas.push(Persona {
            id,
            conversation_id: String::new(),
            display_name: config.persona.display_name,
            subtitle: config.persona.subtitle,
            directory: path.display().to_string(),
            soul: read(&path.join("SOUL.md"))?,
            memory: read(&path.join("MEMORY.md"))?,
            chat: config.chat.or_else(|| app.defaults.chat.clone()),
        });
    }
    personas.sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.id.cmp(&b.id)));
    Ok(Workspace {
        data_root: root.display().to_string(),
        personas,
    })
}
