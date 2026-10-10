use super::{Database, contexts::create_session, state::current};
use crate::{Error, Persona, ResponseAction, ResponseSettings, ResponseUpdate};
use rusqlite::{Connection, OptionalExtension, params};

fn snapshot(db: &Connection, persona: &Persona) -> Result<ResponseSettings, Error> {
    let session: Option<(String, Option<String>)> = db
        .query_row(
            "SELECT id,response_schema FROM model_sessions WHERE persona_id=?1 AND status='active'",
            [&persona.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (session_id, schema) = session
        .map(|(id, schema)| (Some(id), schema))
        .unwrap_or_default();
    Ok(ResponseSettings {
        schema_json: serde_json::to_string_pretty(&persona.response_schema)?,
        initial_state_json: serde_json::to_string_pretty(&persona.initial_state)?,
        current_state_json: serde_json::to_string_pretty(&current(db, &persona.id)?)?,
        session_id,
        active_schema_json: schema
            .map(|text| -> Result<_, Error> {
                Ok(serde_json::to_string_pretty(&serde_json::from_str::<
                    serde_json::Value,
                >(&text)?)?)
            })
            .transpose()?,
    })
}

impl Database {
    pub(crate) fn response_settings(&self, persona: &Persona) -> Result<ResponseSettings, Error> {
        snapshot(&self.connection.lock().expect("database lock"), persona)
    }

    pub(crate) fn apply_response_settings(
        &self,
        persona: &Persona,
        update: &ResponseUpdate,
        publish: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        // Queue acceptance is locked by the caller. Recheck durable state inside the transaction.
        let actual = snapshot(&tx, persona)?;
        if actual.current_state_json != update.expected.current_state_json
            || actual.session_id != update.expected.session_id
            || actual.active_schema_json != update.expected.active_schema_json
        {
            return Err(crate::response_settings::stale());
        }
        if update.action != ResponseAction::InitialDefaultsOnly {
            if update.action == ResponseAction::FreshContextResetState {
                tx.execute(
                    "UPDATE persona_states SET current_state=?2 WHERE persona_id=?1",
                    params![persona.id, serde_json::to_string(&persona.initial_state)?],
                )?;
            }
            let start = tx.query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM persona_events WHERE persona_id=?1",
                [&persona.id],
                |r| r.get::<_, i64>(0),
            )?;
            create_session(&tx, persona, start, "manual")?;
        }
        publish().map_err(|error| Error::SettingsSave(format!("{error} Accepted state and the active context were not changed. Reload the state controls and inspect the character files before retrying.")))?;
        tx.commit().map_err(|_| Error::SettingsSave("Response files were published, but the state/context transaction could not commit. Accepted state and the old context remain unchanged. Reload the state controls, review the saved files, and explicitly apply a fresh context again; open the character folder if files need repair.".into()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_publication_does_not_commit_prepared_reset_or_new_context() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::Core::open(root.path()).unwrap();
        let mut persona = core.workspace().unwrap().personas.remove(0);
        core.fresh_context(&persona.id).unwrap();
        let expected = core.response_settings(&persona.id).unwrap();
        let update = ResponseUpdate {
            schema_json: expected.schema_json.clone(),
            initial_state_json: expected.initial_state_json.clone(),
            expected: expected.clone(),
            action: ResponseAction::FreshContextResetState,
        };
        persona.initial_state = serde_json::json!({"affinity_user":7,"mood":"quiet"});
        persona.response_schema["description"] =
            serde_json::json!("Published before later file failure");
        let schema = std::path::Path::new(&persona.directory).join("response_schema.json");
        let error = core
            .database
            .apply_response_settings(&persona, &update, || {
                // Model the documented per-file boundary: one file has already been replaced.
                std::fs::write(
                    &schema,
                    serde_json::to_string(&persona.response_schema).unwrap(),
                )
                .unwrap();
                Err(Error::SettingsSave(
                    "Saved response_schema.json, but config.toml publication failed.".into(),
                ))
            })
            .unwrap_err()
            .to_string();
        assert!(error.contains("active context were not changed") && error.contains("Reload"));
        let after = core.response_settings(&persona.id).unwrap();
        assert_ne!(after.schema_json, expected.schema_json);
        assert_eq!(after.initial_state_json, expected.initial_state_json);
        assert_eq!(after.current_state_json, expected.current_state_json);
        assert_eq!(after.session_id, expected.session_id);
        assert_eq!(after.active_schema_json, expected.active_schema_json);
    }
}
