#[derive(Clone, uniffi::Record)]
pub struct ConnectionSettingsRecord {
    pub enabled: bool,
    pub model: String,
    pub base_url: String,
    pub api: String,
    pub responses_schema_format: String,
    pub has_api_key: bool,
    pub api_key_env: String,
}
#[derive(Clone, uniffi::Record)]
pub struct PersonaSettingsRecord {
    pub display_name: String,
    pub subtitle: String,
    pub soul: String,
    pub memory: String,
}
#[derive(uniffi::Record)]
pub struct SettingsRecord {
    pub chat: ConnectionSettingsRecord,
    pub context_window_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub persona: Option<PersonaSettingsRecord>,
}
#[derive(uniffi::Enum)]
pub enum CredentialUpdate {
    Keep,
    Replace { value: String },
    Clear,
    Environment { name: String },
}
#[derive(uniffi::Record)]
pub struct SettingsUpdateRecord {
    pub chat: ConnectionSettingsRecord,
    pub credential: CredentialUpdate,
    pub context_window_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub persona: Option<PersonaSettingsRecord>,
}
impl From<pebble_core::Settings> for SettingsRecord {
    fn from(s: pebble_core::Settings) -> Self {
        Self {
            chat: ConnectionSettingsRecord {
                enabled: s.chat.enabled,
                model: s.chat.model,
                base_url: s.chat.base_url,
                api: s.chat.api,
                responses_schema_format: s.chat.responses_schema_format,
                has_api_key: s.chat.has_api_key,
                api_key_env: s.chat.api_key_env,
            },
            context_window_tokens: s.context_window_tokens,
            max_output_tokens: s.max_output_tokens,
            persona: s.persona.map(|p| PersonaSettingsRecord {
                display_name: p.display_name,
                subtitle: p.subtitle,
                soul: p.soul,
                memory: p.memory,
            }),
        }
    }
}
impl From<SettingsUpdateRecord> for pebble_core::SettingsUpdate {
    fn from(s: SettingsUpdateRecord) -> Self {
        Self {
            chat: pebble_core::ChatSettings {
                enabled: s.chat.enabled,
                model: s.chat.model,
                base_url: s.chat.base_url,
                api: s.chat.api,
                responses_schema_format: s.chat.responses_schema_format,
                has_api_key: s.chat.has_api_key,
                api_key_env: s.chat.api_key_env,
            },
            credential: match s.credential {
                CredentialUpdate::Keep => pebble_core::CredentialChange::Keep,
                CredentialUpdate::Replace { value } => {
                    pebble_core::CredentialChange::Replace(value)
                }
                CredentialUpdate::Clear => pebble_core::CredentialChange::Clear,
                CredentialUpdate::Environment { name } => {
                    pebble_core::CredentialChange::Environment(name)
                }
            },
            context_window_tokens: s.context_window_tokens,
            max_output_tokens: s.max_output_tokens,
            persona: s.persona.map(|p| pebble_core::PersonaUpdate {
                display_name: p.display_name,
                subtitle: p.subtitle,
                soul: p.soul,
                memory: p.memory,
            }),
        }
    }
}
