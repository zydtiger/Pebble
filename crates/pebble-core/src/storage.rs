mod jobs;
mod turns;

use crate::{Error, Message, MessageStatus, ModelItem, ModelRequest, Persona, file_error};
use rusqlite::{Connection, OptionalExtension, params};
use std::fs::File;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

pub(crate) struct Database {
    connection: Mutex<Connection>,
    // Exclusive process lifetime ownership makes startup interruption recovery safe.
    _lock: File,
}

impl Database {
    pub(crate) fn open(root: &Path) -> Result<Self, Error> {
        let data = root.join("data");
        std::fs::create_dir_all(&data).map_err(|error| file_error(&data, error))?;
        let lock_path = data.join("app.lock");
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| file_error(&lock_path, error))?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => Error::AlreadyOpen,
            std::fs::TryLockError::Error(error) => file_error(&lock_path, error),
        })?;
        let mut connection = Connection::open(data.join("app.sqlite"))?;
        connection.busy_timeout(std::time::Duration::from_secs(3))?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 4 {
            connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
        }
        let tx = connection.transaction()?;
        if version > 4 {
            return Err(Error::Schema(version));
        }
        if version == 0 {
            tx.execute_batch(
                "CREATE TABLE personas (
                    id TEXT PRIMARY KEY, display_name TEXT NOT NULL, directory TEXT NOT NULL UNIQUE,
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 );
                 CREATE TABLE conversations (
                    id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK (kind = 'direct'),
                    persona_id TEXT NOT NULL UNIQUE REFERENCES personas(id),
                    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 );",
            )?;
        }
        if version < 2 {
            tx.execute_batch(include_str!("storage/schema.sql"))?;
            tx.execute_batch("PRAGMA user_version = 2;")?;
        }
        if version < 3 {
            tx.execute_batch("ALTER TABLE model_sessions ADD COLUMN base_url TEXT NOT NULL DEFAULT '';
                ALTER TABLE model_sessions ADD COLUMN event_start_sequence INTEGER NOT NULL DEFAULT 0;
                PRAGMA user_version = 3;")?;
        }
        if version < 4 {
            tx.execute_batch(include_str!("storage/queue-migration.sql"))?;
            tx.execute_batch("PRAGMA user_version = 4;")?;
        }
        // Durable deltas remain visible, but unfinished outputs are never accepted model items.
        tx.execute_batch(
            "UPDATE messages SET status = 'interrupted' WHERE status = 'streaming';
             UPDATE model_calls SET status = 'interrupted', error = 'Application stopped before completion',
                 finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE status IN ('streaming', 'queued');"
        )?;
        if tx.prepare("PRAGMA foreign_key_check")?.exists([])? {
            return Err(Error::Database(rusqlite::Error::InvalidQuery));
        }
        tx.commit()?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Self {
            connection: Mutex::new(connection),
            _lock: lock,
        })
    }

    pub(crate) fn resolve_conversations(&self, personas: &mut [Persona]) -> Result<(), Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        for persona in personas {
            tx.execute(
                "INSERT INTO personas (id, display_name, directory) VALUES (?1, ?2, ?1)
                 ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name",
                params![persona.id, persona.display_name],
            )?;
            tx.execute(
                "INSERT INTO conversations (id, kind, persona_id) VALUES (?1, 'direct', ?2)
                 ON CONFLICT(persona_id) DO NOTHING",
                params![Uuid::new_v4().to_string(), persona.id],
            )?;
            let (protocol, model, base_url) = super::connection::identity(persona.chat.as_ref());
            persona.connection_label = match persona.chat.as_ref() {
                None => "Local demo · No model connected".into(),
                Some(chat) if chat.api == "chat_completions" => format!("Live · {}", chat.model),
                Some(chat) => format!("{} · Not implemented", chat.api),
            };
            let prior: Option<(String, String, String, i64)> = tx.query_row(
                "SELECT protocol, model, base_url, event_start_sequence FROM model_sessions WHERE persona_id = ?1 AND status = 'active'",
                [&persona.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .optional()?;
            persona.context_notice = match prior {
                Some((p, m, b, start)) if p != protocol || m != model || b != base_url || start > 0 =>
                    "Connection changed: earlier messages stay visible; this model uses a fresh context.".into(),
                _ => String::new(),
            };
            persona.conversation_id = tx.query_row(
                "SELECT id FROM conversations WHERE persona_id = ?1",
                [&persona.id],
                |row| row.get(0),
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn messages(&self, conversation_id: &str) -> Result<Vec<Message>, Error> {
        let connection = self.connection.lock().expect("database lock");
        let mut query = connection.prepare(
            "SELECT m.id, m.conversation_id, m.sequence, m.sender_kind, m.sender_id, m.content,
                CASE WHEN m.status = 'streaming' AND j.status = 'queued' THEN 'queued' ELSE m.status END,
                m.created_at FROM messages m LEFT JOIN model_calls j ON j.reply_message_id = m.id
                WHERE m.conversation_id = ?1 ORDER BY m.sequence"
        )?;
        Ok(query
            .query_map([conversation_id], read_message)?
            .collect::<Result<_, _>>()?)
    }

    pub(crate) fn request(&self, call_id: &str) -> Result<ModelRequest, Error> {
        let connection = self.connection.lock().expect("database lock");
        let (mut request, last): (ModelRequest, i64) = connection.query_row(
            "SELECT c.session_id, c.conversation_id, c.trigger_message_id, c.event_cutoff,
                    s.protocol, s.model, c.input_last_sequence
             FROM model_calls c JOIN model_sessions s ON s.id = c.session_id WHERE c.id = ?1",
            [call_id],
            |row| {
                Ok((
                    ModelRequest {
                        call_id: call_id.into(),
                        session_id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        trigger_message_id: row.get(2)?,
                        event_cutoff: row.get(3)?,
                        protocol: row.get(4)?,
                        model: row.get(5)?,
                        items: Vec::new(),
                    },
                    row.get(6)?,
                ))
            },
        )?;
        let mut query = connection.prepare(
            "SELECT id, sequence, payload FROM model_items WHERE session_id = ?1 AND sequence <= ?2 ORDER BY sequence"
        )?;
        request.items = query
            .query_map(params![request.session_id, last], |row| {
                Ok(ModelItem {
                    id: row.get(0)?,
                    sequence: row.get(1)?,
                    payload: row.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(request)
    }
}

fn read_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let status: String = row.get(6)?;
    let status = match status.as_str() {
        "complete" => MessageStatus::Complete,
        "streaming" => MessageStatus::Streaming,
        "queued" => MessageStatus::Queued,
        "cancelled" => MessageStatus::Cancelled,
        "interrupted" => MessageStatus::Interrupted,
        "failed" => MessageStatus::Failed,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        sequence: row.get(2)?,
        sender_kind: row.get(3)?,
        sender_id: row.get(4)?,
        content: row.get(5)?,
        status,
        created_at: row.get(7)?,
    })
}
