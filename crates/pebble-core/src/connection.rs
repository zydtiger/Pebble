use crate::{ChatConnection, Error};

pub(crate) fn identity(chat: Option<&ChatConnection>) -> (String, String, String) {
    match chat {
        None => ("pebble_demo_v1".into(), "local-demo".into(), String::new()),
        Some(chat) => (
            chat.api.clone(),
            chat.model.clone(),
            chat.base_url.trim_end_matches('/').into(),
        ),
    }
}

pub(crate) fn credential(chat: &ChatConnection) -> Result<Option<String>, Error> {
    let key = if let Some(name) = &chat.api_key_env {
        std::env::var(name).map_err(|_| {
            Error::Generation("The configured API key environment variable is unavailable.".into())
        })?
    } else {
        chat.api_key.clone().unwrap_or_default()
    };
    if key.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(key))
    }
}
