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
    pub rooms: Vec<crate::Room>,
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
    pub classifier: Option<ClassifierConnection>,
    pub context: crate::ContextBudget,
    pub response_schema: serde_json::Value,
    pub initial_state: serde_json::Value,
    pub connection_label: String,
    pub context_notice: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClassifierConnection {
    #[serde(flatten)]
    pub connection: ChatConnection,
    pub request_profile: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct ChatConnection {
    pub model: String,
    pub base_url: String,
    pub api: String,
    pub context_len: u64,
    pub max_output_tokens: u64,
    pub api_key: Option<String>,
    pub api_key_env: Option<String>,
    /// Responses standard text.format or the explicitly selected llama.cpp extension.
    pub responses_schema_format: Option<String>,
}

impl std::fmt::Debug for ChatConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatConnection")
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("api", &self.api)
            .field("context_len", &self.context_len)
            .field("max_output_tokens", &self.max_output_tokens)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("api_key_env", &self.api_key_env)
            .field("responses_schema_format", &self.responses_schema_format)
            .finish()
    }
}

#[derive(Deserialize, Default)]
struct Defaults {
    chat: Option<ChatConnection>,
    classifier: Option<ClassifierConnection>,
}

#[derive(Deserialize)]
struct AppConfig {
    #[serde(default)]
    defaults: Defaults,
}

#[derive(Deserialize)]
struct PersonaConfig {
    persona: Identity,
    response: ResponseConfig,
    chat: Option<ChatConnection>,
    classifier: Option<ClassifierConnection>,
}

#[derive(Deserialize)]
struct ResponseConfig {
    initial_state: serde_json::Value,
}

#[derive(Deserialize)]
struct Identity {
    display_name: String,
    #[serde(default)]
    subtitle: String,
}

fn read(path: &Path, overrides: &[(std::path::PathBuf, String)]) -> Result<String, Error> {
    if let Some((_, text)) = overrides.iter().find(|(p, _)| p == path) {
        return Ok(text.clone());
    }
    fs::read_to_string(path).map_err(|error| file_error(path, error))
}

fn parse<T: serde::de::DeserializeOwned>(
    path: &Path,
    overrides: &[(std::path::PathBuf, String)],
) -> Result<T, Error> {
    let text = read(path, overrides)?;
    let document: toml::Value = toml::from_str(&text).map_err(|_| Error::Configuration {
        path: path.display().to_string(),
        detail: "Invalid TOML configuration; repair this file.".into(),
    })?;
    for table in [Some(&document), document.get("defaults")]
        .into_iter()
        .flatten()
    {
        if ["context", "classifier_context"]
            .iter()
            .any(|key| table.get(key).is_some())
        {
            return Err(Error::Configuration { path: path.display().to_string(), detail: "Obsolete context table: move context_len and max_output_tokens into each [chat]/[classifier] or [defaults.chat]/[defaults.classifier] connection and remove [context]/[classifier_context] tables. Persona connections inherit as a whole.".into() });
        }
        for name in ["chat", "classifier"] {
            if let Some(connection) = table.get(name) {
                if connection.get("context_window_tokens").is_some() {
                    return Err(Error::Configuration {
                        path: path.display().to_string(),
                        detail: format!(
                            "{name}.context_window_tokens is obsolete; use context_len and max_output_tokens in this connection."
                        ),
                    });
                }
                for limit in ["context_len", "max_output_tokens"] {
                    if connection
                        .get(limit)
                        .and_then(toml::Value::as_integer)
                        .is_none_or(|v| v <= 0)
                    {
                        return Err(Error::Configuration {
                            path: path.display().to_string(),
                            detail: format!(
                                "{name}.{limit} is required and must be a positive whole number. A configured connection requires its own context_len and max_output_tokens."
                            ),
                        });
                    }
                }
            }
        }
    }
    toml::from_str(&text).map_err(|error| Error::Configuration {
        path: path.display().to_string(),
        detail: format!(
            "Invalid TOML configuration at {}",
            error
                .span()
                .map(|span| format!("byte {}", span.start))
                .unwrap_or_else(|| "unknown location".into())
        ),
    })
}

fn validate_connection(connection: &ChatConnection, path: &Path) -> Result<(), Error> {
    let invalid = |detail: &str| Error::Configuration {
        path: path.display().to_string(),
        detail: detail.into(),
    };
    if connection.max_output_tokens == 0 || connection.max_output_tokens >= connection.context_len {
        return Err(invalid(
            "chat.context_len and chat.max_output_tokens must be positive; max_output_tokens must be smaller than context_len",
        ));
    }
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
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "chat.base_url must not include credentials, a query, or a fragment",
        ));
    }
    if !matches!(connection.api.as_str(), "chat_completions" | "responses") {
        return Err(invalid("chat.api must be chat_completions or responses"));
    }
    if connection
        .responses_schema_format
        .as_ref()
        .is_some_and(|format| {
            connection.api != "responses"
                || !matches!(format.as_str(), "text_format" | "response_format")
        })
    {
        return Err(invalid(
            "chat.responses_schema_format applies only to responses and must be text_format or response_format",
        ));
    }
    if connection.api_key_env.as_ref().is_some_and(|name| {
        name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }) {
        return Err(invalid(
            "chat.api_key_env must name an environment variable",
        ));
    }
    if connection
        .api_key
        .as_ref()
        .is_some_and(|key| !key.trim().is_empty())
        && connection.api_key_env.is_some()
    {
        return Err(invalid(
            "Set either chat.api_key or chat.api_key_env, not both",
        ));
    }
    Ok(())
}

fn validate_classifier(connection: &ClassifierConnection, path: &Path) -> Result<(), Error> {
    validate_connection(&connection.connection, path).map_err(|error| match error {
        Error::Configuration { path, detail } => Error::Configuration {
            path,
            detail: detail.replace("chat.", "classifier."),
        },
        error => error,
    })?;
    if connection
        .request_profile
        .as_deref()
        .is_some_and(|profile| {
            profile != "qwen_non_thinking" || connection.connection.api != "chat_completions"
        })
    {
        return Err(Error::Configuration { path: path.display().to_string(), detail: "classifier.request_profile supports only qwen_non_thinking with chat_completions; omit it for standard requests.".into() });
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
        (
            persona.join("response_schema.json"),
            crate::structured::DEFAULT_SCHEMA,
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
    load_candidate(root, &[])
}

pub(crate) fn load_candidate(
    root: &Path,
    overrides: &[(std::path::PathBuf, String)],
) -> Result<Workspace, Error> {
    let app_path = root.join("config.toml");
    let app: AppConfig = parse(&app_path, overrides)?;
    if let Some(connection) = &app.defaults.chat {
        validate_connection(connection, &app_path)?;
    }
    if let Some(connection) = &app.defaults.classifier {
        validate_classifier(connection, &app_path)?;
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
        let config: PersonaConfig = parse(&config_path, overrides)?;
        if config.persona.display_name.trim().is_empty() {
            return Err(Error::Configuration {
                path: config_path.display().to_string(),
                detail: "persona.display_name must not be empty".into(),
            });
        }
        if let Some(connection) = &config.chat {
            validate_connection(connection, &config_path)?;
        }
        if let Some(connection) = &config.classifier {
            validate_classifier(connection, &config_path)?;
        }
        personas.push(Persona {
            id,
            conversation_id: String::new(),
            display_name: config.persona.display_name,
            subtitle: config.persona.subtitle,
            directory: path.display().to_string(),
            soul: read(&path.join("SOUL.md"), overrides)?,
            memory: read(&path.join("MEMORY.md"), overrides)?,
            connection_label: String::new(),
            context_notice: String::new(),
            chat: config.chat.clone().or_else(|| app.defaults.chat.clone()),
            classifier: config
                .classifier
                .or_else(|| app.defaults.classifier.clone()),
            context: config
                .chat
                .as_ref()
                .or(app.defaults.chat.as_ref())
                .map(ChatConnection::budget)
                .unwrap_or_default(),
            response_schema: crate::structured::parse(
                &path.join("response_schema.json"),
                &read(&path.join("response_schema.json"), overrides)?,
                &config.response.initial_state,
            )?,
            initial_state: config.response.initial_state,
        });
    }
    personas.sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.id.cmp(&b.id)));
    Ok(Workspace {
        data_root: root.display().to_string(),
        personas,
        rooms: Vec::new(),
    })
}

impl ChatConnection {
    pub fn budget(&self) -> crate::ContextBudget {
        crate::ContextBudget {
            context_len: self.context_len,
            max_output_tokens: self.max_output_tokens,
        }
    }
}
