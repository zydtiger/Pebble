#[derive(Clone, uniffi::Record)]
pub struct ResponseSettingsRecord {
    pub schema_json: String,
    pub initial_state_json: String,
    pub current_state_json: String,
    pub session_id: Option<String>,
    pub active_schema_json: Option<String>,
}
#[derive(Clone, uniffi::Enum)]
pub enum ResponseSettingsAction {
    InitialDefaultsOnly,
    FreshContextKeepState,
    FreshContextResetState,
}
#[derive(uniffi::Record)]
pub struct ResponseSettingsUpdateRecord {
    pub expected: ResponseSettingsRecord,
    pub schema_json: String,
    pub initial_state_json: String,
    pub action: ResponseSettingsAction,
}
impl From<pebble_core::ResponseSettings> for ResponseSettingsRecord {
    fn from(value: pebble_core::ResponseSettings) -> Self {
        Self {
            schema_json: value.schema_json,
            initial_state_json: value.initial_state_json,
            current_state_json: value.current_state_json,
            session_id: value.session_id,
            active_schema_json: value.active_schema_json,
        }
    }
}
impl From<ResponseSettingsRecord> for pebble_core::ResponseSettings {
    fn from(value: ResponseSettingsRecord) -> Self {
        Self {
            schema_json: value.schema_json,
            initial_state_json: value.initial_state_json,
            current_state_json: value.current_state_json,
            session_id: value.session_id,
            active_schema_json: value.active_schema_json,
        }
    }
}
impl From<ResponseSettingsUpdateRecord> for pebble_core::ResponseUpdate {
    fn from(value: ResponseSettingsUpdateRecord) -> Self {
        Self {
            expected: value.expected.into(),
            schema_json: value.schema_json,
            initial_state_json: value.initial_state_json,
            action: match value.action {
                ResponseSettingsAction::InitialDefaultsOnly => {
                    pebble_core::ResponseAction::InitialDefaultsOnly
                }
                ResponseSettingsAction::FreshContextKeepState => {
                    pebble_core::ResponseAction::FreshContextKeepState
                }
                ResponseSettingsAction::FreshContextResetState => {
                    pebble_core::ResponseAction::FreshContextResetState
                }
            },
        }
    }
}
