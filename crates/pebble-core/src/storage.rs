mod codec;
mod contexts;
mod delegation;
pub(crate) mod image_generation;
#[cfg(test)]
mod image_generation_tests;
pub(crate) mod images;
#[cfg(test)]
mod images_tests;
mod items;
mod jobs;
pub(crate) mod maintenance;
mod membership;
pub(crate) mod memory;
mod participation;
mod response_settings;
mod retry;
mod rooms;
pub(crate) mod rounds;
mod state;
mod turns;
pub(crate) use turns::input_message;

use crate::{Error, Message, MessageStatus, Persona, file_error};
use rusqlite::{Connection, OptionalExtension, params};
use std::fs::File;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

pub(crate) struct Database {
    connection: Mutex<Connection>,
    root: std::path::PathBuf,
    pub(crate) image_generation: Mutex<std::sync::Weak<crate::image_generation::Manager>>,
    pub(crate) codex: Mutex<std::sync::Weak<crate::codex::Manager>>,
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
        if version < 8 {
            connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
        }
        let tx = connection.transaction()?;
        if version > 14 {
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
        if version < 5 {
            tx.execute_batch(include_str!("storage/codec-migration.sql"))?;
            tx.execute_batch("PRAGMA user_version = 5;")?;
        }
        if version < 6 {
            tx.execute_batch("ALTER TABLE model_calls ADD COLUMN retry_of TEXT REFERENCES model_calls(id); PRAGMA user_version=6;")?;
        }
        if version < 7 {
            tx.execute_batch(
                "CREATE TABLE persona_states (
                persona_id TEXT PRIMARY KEY REFERENCES personas(id), current_state TEXT NOT NULL);
                ALTER TABLE model_sessions ADD COLUMN response_schema TEXT;
                ALTER TABLE model_calls ADD COLUMN state_before TEXT;
                ALTER TABLE model_calls ADD COLUMN validation_errors TEXT NOT NULL DEFAULT '[]';
                PRAGMA user_version=7;",
            )?;
        }
        if version < 8 {
            tx.execute_batch(include_str!("storage/groups-migration.sql"))?;
        }
        if version < 9 {
            tx.execute_batch(include_str!("storage/participation-migration.sql"))?;
        }
        if version < 10 {
            tx.execute_batch(include_str!("storage/rounds-migration.sql"))?;
        }
        if version < 11 {
            tx.execute_batch(include_str!("storage/memory-migration.sql"))?;
        }
        if version < 12 {
            tx.execute_batch("CREATE TABLE image_assets(id TEXT PRIMARY KEY,mime TEXT NOT NULL,width INTEGER NOT NULL,height INTEGER NOT NULL,source_bytes INTEGER NOT NULL,model_bytes INTEGER NOT NULL,source_name TEXT NOT NULL,reference TEXT NOT NULL);
                CREATE TABLE message_images(message_id TEXT NOT NULL REFERENCES messages(id),asset_id TEXT NOT NULL REFERENCES image_assets(id),position INTEGER NOT NULL,PRIMARY KEY(message_id,position),UNIQUE(message_id,asset_id)); PRAGMA user_version=12;")?;
        }
        if version < 13 {
            tx.execute_batch(include_str!("storage/delegation-migration.sql"))?;
        }
        if version < 14 {
            tx.execute_batch(include_str!("storage/image-generation-migration.sql"))?;
        }
        tx.execute_batch("UPDATE group_reply_slots SET status='interrupted' WHERE status IN ('pending','admitted'); UPDATE group_rounds SET status='interrupted',continuation=0 WHERE status IN ('active','superseded');")?;
        tx.execute("UPDATE participation_decisions SET status='interrupted',error='Application stopped before participation completed; mention a character to continue.' WHERE status='pending'", [])?;
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
        let database = Self {
            connection: Mutex::new(connection),
            root: root.to_path_buf(),
            codex: Mutex::new(std::sync::Weak::new()),
            image_generation: Mutex::new(std::sync::Weak::new()),
            _lock: lock,
        };
        database.recover_memory(root)?;
        database.recover_maintenance()?;
        database.recover_delegations()?;
        database.recover_image_generations()?;
        Ok(database)
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
            super::structured::validate_state(&persona.response_schema, &persona.initial_state)
                .map_err(Error::Generation)?;
            tx.execute("INSERT INTO persona_states(persona_id,current_state) VALUES (?1,?2) ON CONFLICT(persona_id) DO NOTHING", params![persona.id, serde_json::to_string(&persona.initial_state)?])?;
            let (protocol, model, base_url) = super::connection::identity(persona.chat.as_ref());
            persona.connection_label = match persona.chat.as_ref() {
                None => "Local demo · No model connected".into(),
                Some(chat) if chat.api == "responses" => {
                    format!("Live · {} · Responses", chat.model)
                }
                Some(chat) => format!("Live · {}", chat.model),
            };
            let prior: Option<(String, String, String, i64, String, Option<String>)> = tx.query_row(
                "SELECT protocol, model, base_url, event_start_sequence, context_reason, response_schema FROM model_sessions WHERE persona_id = ?1 AND status = 'active'",
                [&persona.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
                .optional()?;
            persona.context_notice = match prior {
                Some((_, _, _, _, _, None)) =>
                    "Legacy model context: structured replies and persistent state require Start fresh context. Existing context stays unchanged until then.".into(),
                Some((_, _, _, _, reason, _)) if reason == "manual" =>
                    "Fresh model context: earlier messages stay visible and are excluded from new requests.".into(),
                Some((p, m, b, start, _, _)) if p != protocol || m != model || b != base_url || start > 0 =>
                    "Connection changed: earlier messages stay visible; this model uses a fresh context.".into(),
                _ => String::new(),
            };
            let capability:Option<bool>=tx.query_row("SELECT memory_tools FROM model_sessions WHERE persona_id=?1 AND status='active'",[&persona.id],|r|r.get(0)).optional()?;
            if capability == Some(false) {
                persona.context_notice.push_str(" This preserved context has no memory tool capability. Start fresh context to review memory and enable it; the existing prefix is unchanged.");
            }
            let reason:Option<String>=tx.query_row("SELECT context_reason FROM model_sessions WHERE persona_id=?1 AND status='active'",[&persona.id],|r|r.get(0)).optional()?;
            if reason.as_deref() == Some("memory") {
                persona.context_notice="Fresh model context started; earlier messages and native history remain saved.".into();
            }
            if let Some(detail)=tx.query_row("SELECT detail FROM context_maintenance WHERE persona_id=?1 AND status IN ('failed','cancelled','interrupted') AND rowid=(SELECT MAX(rowid) FROM context_maintenance WHERE persona_id=?1)",[&persona.id],|r|r.get::<_,String>(0)).optional()? {persona.context_notice.push_str(&format!(" {detail}"));}
            let delegation:Option<bool>=tx.query_row("SELECT delegation_tools FROM model_sessions WHERE persona_id=?1 AND status='active'",[&persona.id],|r|r.get(0)).optional()?;
            if delegation == Some(false) {
                persona.context_notice.push_str(" This preserved context has no delegation capability. Start fresh context to enable Codex tasks; the frozen prefix is unchanged.");
            }
            let image_capability:Option<bool>=tx.query_row("SELECT image_generation_tools FROM model_sessions WHERE persona_id=?1 AND status='active'",[&persona.id],|r|r.get(0)).optional()?;
            if image_capability == Some(false) {
                persona.context_notice.push_str(" Start fresh context to enable image generation; the existing prefix stays unchanged.");
            }
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
                m.created_at, COALESCE(j.error, ''),
                (j.status IN ('failed','cancelled','interrupted')
                  AND (j.membership_id IS NULL OR EXISTS(SELECT 1 FROM conversation_memberships member WHERE member.id=j.membership_id AND member.left_publication IS NULL))
                  AND (NOT EXISTS(SELECT 1 FROM delegated_tasks task WHERE json_extract(task.record,'$.report_call')=j.id OR EXISTS(SELECT 1 FROM json_each(task.record,'$.reports') r WHERE json_extract(r.value,'$.call')=j.id)) OR EXISTS(SELECT 1 FROM delegated_tasks task WHERE json_extract(task.record,'$.report_call')=j.id AND json_extract(task.record,'$.report_state')='failed')) AND NOT EXISTS(SELECT 1 FROM image_generations image WHERE EXISTS(SELECT 1 FROM json_each(image.record,'$.reports') r WHERE json_extract(r.value,'$.call')=j.id) AND (json_extract(image.record,'$.invalidated') OR json_extract(image.record,'$.report_call') IS NOT j.id OR json_extract(image.record,'$.publication_status')!='failed')) AND NOT EXISTS(SELECT 1 FROM model_calls newer WHERE newer.persona_id=j.persona_id AND newer.persona_sequence>j.persona_sequence)
                  AND NOT EXISTS(SELECT 1 FROM model_calls busy WHERE busy.persona_id=j.persona_id AND busy.status IN ('queued','streaming')) AND NOT EXISTS(SELECT 1 FROM context_maintenance maintenance WHERE maintenance.persona_id=j.persona_id AND maintenance.status='running') AND NOT EXISTS(SELECT 1 FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE d.persona_id=j.persona_id AND s.status IN ('pending','admitted')))
                FROM messages m LEFT JOIN model_calls j ON j.reply_message_id = m.id
                WHERE m.conversation_id = ?1 ORDER BY m.sequence"
        )?;
        let mut messages: Vec<Message> = query
            .query_map([conversation_id], |row| {
                let mut message = read_message(row)?;
                message.failure_detail = row.get(8)?;
                message.can_retry = row.get::<_, Option<bool>>(9)?.unwrap_or(false);
                Ok(message)
            })?
            .collect::<Result<_, _>>()?;
        for message in &mut messages {
            message.images = images::assets(&connection, &message.id)?;
        }
        Ok(messages)
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
        failure_detail: String::new(),
        can_retry: false,
        images: Vec::new(),
    })
}
