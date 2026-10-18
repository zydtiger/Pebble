use crate::{Error, file_error};
use std::path::Path;
use uuid::Uuid;

pub(crate) fn create(root: &Path, name: &str) -> Result<String, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Configuration {
            path: root.join("personas").display().to_string(),
            detail: "A character needs a display name.".into(),
        });
    }
    let id = Uuid::new_v4().to_string();
    // Publish all character files together so readers never see an incomplete character.
    let staging = root.join(format!(".new-persona-{id}"));
    std::fs::create_dir(&staging).map_err(|e| file_error(&staging, e))?;
    let result = (|| {
        let mut identity = toml::Table::new();
        identity.insert("display_name".into(), toml::Value::String(name.into()));
        identity.insert(
            "subtitle".into(),
            toml::Value::String("A familiar voice, in your own words.".into()),
        );
        let mut document = toml::Table::new();
        document.insert("persona".into(), toml::Value::Table(identity));
        let config = toml::to_string(&document)
            .map_err(|_| Error::Generation("Could not encode character settings.".into()))?;
        for (file, text) in [
            (
                "config.toml",
                format!(
                    "{config}{}{}",
                    crate::structured::DEFAULT_INITIAL,
                    crate::activity::settings::DEFAULT_CONFIG
                ),
            ),
            (
                "response_schema.json",
                crate::structured::DEFAULT_SCHEMA.into(),
            ),
            (
                "SOUL.md",
                format!(
                    "# {name}\n\nYou are {name}, a thoughtful conversation partner. Be warm, curious, and honest.\n"
                ),
            ),
            ("MEMORY.md", include_str!("../assets/MEMORY.md").into()),
        ] {
            let path = staging.join(file);
            std::fs::write(&path, text).map_err(|e| file_error(&path, e))?;
        }
        let destination = root.join("personas").join(&id);
        std::fs::rename(&staging, &destination).map_err(|e| file_error(&destination, e))?;
        Ok(id)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}
