use crate::{ChatConnection, Error};

impl ChatConnection {
    /// DeepSeek Chat Completions accepts JSON mode, but rejects JSON Schema mode.
    /// Match the API host, not the model name (which other servers can also serve).
    pub(crate) fn uses_json_object(&self) -> bool {
        self.api == "chat_completions"
            && url::Url::parse(&self.base_url)
                .is_ok_and(|url| url.host_str() == Some("api.deepseek.com"))
    }
}

pub(crate) fn identity(chat: Option<&ChatConnection>) -> (String, String, String) {
    match chat {
        None => ("pebble_demo_v1".into(), "local-demo".into(), String::new()),
        Some(chat) => (
            chat.api.clone(),
            chat.model.clone(),
            format!(
                "{}{}",
                chat.base_url.trim_end_matches('/'),
                crate::request_parameters::template_identity(&chat.request)
            ),
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
