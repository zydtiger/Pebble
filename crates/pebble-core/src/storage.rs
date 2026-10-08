use crate::{Error, Persona, file_error};
use rusqlite::{Connection, params};
use std::path::Path;
use uuid::Uuid;

pub(crate) fn resolve_conversations(root: &Path, personas: &mut [Persona]) -> Result<(), Error> {
    let data = root.join("data");
    std::fs::create_dir_all(&data).map_err(|error| file_error(&data, error))?;
    let mut connection = Connection::open(data.join("app.sqlite"))?;
    connection.busy_timeout(std::time::Duration::from_secs(3))?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS personas (
             id TEXT PRIMARY KEY,
             display_name TEXT NOT NULL,
             directory TEXT NOT NULL UNIQUE,
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         );
         CREATE TABLE IF NOT EXISTS conversations (
             id TEXT PRIMARY KEY,
             kind TEXT NOT NULL CHECK (kind = 'direct'),
             persona_id TEXT NOT NULL UNIQUE REFERENCES personas(id),
             created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         );
         PRAGMA user_version = 1;",
    )?;
    for persona in personas {
        transaction.execute(
            "INSERT INTO personas (id, display_name, directory) VALUES (?1, ?2, ?1)
             ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name",
            params![persona.id, persona.display_name],
        )?;
        transaction.execute(
            "INSERT INTO conversations (id, kind, persona_id) VALUES (?1, 'direct', ?2)
             ON CONFLICT(persona_id) DO NOTHING",
            params![Uuid::new_v4().to_string(), persona.id],
        )?;
        persona.conversation_id = transaction.query_row(
            "SELECT id FROM conversations WHERE persona_id = ?1",
            [&persona.id],
            |row| row.get(0),
        )?;
    }
    transaction.commit()?;
    Ok(())
}
