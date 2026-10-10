use super::Database;
use crate::{Error, file_error};
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

pub(super) fn current(db: &Connection, persona: &str) -> Result<Value, Error> {
    let text: String = db.query_row(
        "SELECT current_state FROM persona_states WHERE persona_id=?1",
        [persona],
        |r| r.get(0),
    )?;
    Ok(serde_json::from_str(&text)?)
}

impl Database {
    /// Only registered legacy personas with no durable state get missing contract defaults.
    /// Publish the config last. Existing schema bytes and existing TOML values stay untouched.
    /// If an upgrade is interrupted before state initialization, reopening resumes it.
    pub(crate) fn upgrade_persona_contracts(&self, root: &Path) -> Result<(), Error> {
        let db = self.connection.lock().expect("database lock");
        let mut query = db.prepare("SELECT id FROM personas WHERE NOT EXISTS(SELECT 1 FROM persona_states s WHERE s.persona_id=personas.id)")?;
        for id in query.query_map([], |r| r.get::<_, String>(0))? {
            let directory = root.join("personas").join(id?);
            let path = directory.join("config.toml");
            let text = std::fs::read_to_string(&path).map_err(|e| file_error(&path, e))?;
            let mut document: toml_edit::DocumentMut =
                text.parse().map_err(|_| Error::Configuration {
                    path: path.display().to_string(),
                    detail: "Repair invalid legacy TOML before upgrading the response contract."
                        .into(),
                })?;
            for (field, item) in [
                ("response", document.get("response")),
                (
                    "response.initial_state",
                    document
                        .get("response")
                        .and_then(|r| r.get("initial_state")),
                ),
            ] {
                if item.is_some_and(|item| item.as_table_like().is_none()) {
                    return Err(Error::Configuration {
                        path: path.display().to_string(),
                        detail: format!(
                            "Repair {field}: expected a TOML table before upgrading the response contract."
                        ),
                    });
                }
            }
            let schema_path = directory.join("response_schema.json");
            if !schema_path.exists() {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&schema_path)
                    .map_err(|e| file_error(&schema_path, e))?;
                file.write_all(crate::structured::DEFAULT_SCHEMA.as_bytes())
                    .map_err(|e| file_error(&schema_path, e))?;
            }
            if document
                .get("response")
                .and_then(|r| r.get("initial_state"))
                .is_none()
            {
                let defaults: toml_edit::DocumentMut = crate::structured::DEFAULT_INITIAL
                    .parse()
                    .expect("default initial state");
                document["response"]["initial_state"] =
                    defaults["response"]["initial_state"].clone();
                super::super::settings::files::publish(&[(path.clone(), document.to_string())])?;
            }
        }
        Ok(())
    }

    pub(crate) fn validation_error(&self, call: &str, detail: &str) -> Result<(), Error> {
        let db = self.connection.lock().expect("database lock");
        let saved: String = db.query_row(
            "SELECT validation_errors FROM model_calls WHERE id=?1",
            [call],
            |r| r.get(0),
        )?;
        let mut errors: Vec<String> = serde_json::from_str(&saved)?;
        errors.push(detail.into());
        db.execute(
            "UPDATE model_calls SET validation_errors=?2 WHERE id=?1",
            rusqlite::params![call, serde_json::to_string(&errors)?],
        )?;
        Ok(())
    }
}
