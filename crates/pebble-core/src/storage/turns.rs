use super::{Database, read_message};
use crate::{Error, Message, MessageStatus, Persona, history::Turn};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

impl Database {
    pub(crate) fn activate_turn(&self, persona: &Persona, pending: &Turn) -> Result<Turn, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        let (protocol, model, base_url) = crate::connection::identity(persona.chat.as_ref());
        let existing: Option<(String, String, String, String, i64)> = tx.query_row(
            "SELECT id, protocol, model, base_url, event_start_sequence FROM model_sessions WHERE persona_id = ?1 AND status = 'active'",
            [&persona.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .optional()?;
        let compatible = existing
            .as_ref()
            .is_some_and(|(_, p, m, b, _)| p == &protocol && m == &model && b == &base_url);
        let (session_id, event_start) = if compatible {
            let (id, _, _, _, start) = existing.expect("compatible session");
            (id, start)
        } else {
            tx.execute("UPDATE model_sessions SET status = 'retired' WHERE persona_id = ?1 AND status = 'active'", [&persona.id])?;
            let start = pending.receipt_cutoff - 1;
            let id = super::contexts::create_session(&tx, persona, start, "connection")?;
            (id, start)
        };
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM model_calls WHERE session_id = ?1 AND status = 'streaming')",
                        [&session_id], |row| row.get::<_, bool>(0))? {
            return Err(Error::Busy);
        }
        if let Some(original) = &pending.retry_of {
            let frozen: Option<(String, i64, i64)> = tx.query_row(
                "WITH RECURSIVE attempts(id,retry_of,session_id,input_last_sequence,event_cutoff,depth) AS (
                    SELECT id,retry_of,session_id,input_last_sequence,event_cutoff,0 FROM model_calls WHERE id=?1
                    UNION ALL SELECT c.id,c.retry_of,c.session_id,c.input_last_sequence,c.event_cutoff,a.depth+1
                    FROM model_calls c JOIN attempts a ON c.id=a.retry_of)
                 SELECT session_id,input_last_sequence,event_cutoff FROM attempts WHERE session_id IS NOT NULL AND input_last_sequence IS NOT NULL ORDER BY depth LIMIT 1",
                [original], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if let Some((source, last, cutoff)) = frozen
                && source == session_id
            {
                tx.execute("UPDATE model_calls SET session_id=?2,input_last_sequence=?3,event_cutoff=?4,status='streaming' WHERE id=?1 AND status='queued'",params![pending.call_id,session_id,last,cutoff])?;
                let mut turn = pending.clone();
                turn.session_id = session_id;
                turn.reply = message(&tx, &pending.reply.id)?;
                tx.commit()?;
                return Ok(turn);
            }
        }
        let user_id = pending.user.id.clone();
        let reply_id = pending.reply.id.clone();

        let cutoff: i64 = tx.query_row(
            "SELECT MAX(e.sequence) FROM persona_events e WHERE e.persona_id = ?1 AND
                (e.sequence <= ?2 OR EXISTS (SELECT 1 FROM model_item_events x JOIN model_items i ON i.id = x.item_id WHERE x.event_id = e.id AND i.session_id = ?3))",
            params![persona.id, pending.receipt_cutoff, session_id],
            |row| row.get(0),
        )?;
        let mut events = Vec::new();
        let mut event_ids = Vec::new();
        {
            let mut query = tx.prepare(
                "SELECT e.id, e.sequence, e.conversation_id, e.message_id, m.sender_kind, m.sender_id, m.content
                 FROM persona_events e JOIN messages m ON m.id = e.message_id
                 WHERE e.persona_id = ?1 AND e.sequence <= ?2 AND (e.sequence > ?4 OR (?6 AND e.message_id=?7)) AND e.kind = 'received' AND e.sequence <= ?5
                 AND NOT EXISTS (SELECT 1 FROM model_calls c WHERE c.trigger_message_id = e.message_id AND c.session_id IS NULL AND c.status IN ('cancelled', 'interrupted', 'failed') AND NOT EXISTS (SELECT 1 FROM model_calls live WHERE live.trigger_message_id=e.message_id AND (live.session_id IS NOT NULL OR live.status IN ('queued','streaming'))))
                 AND NOT EXISTS (
                     SELECT 1 FROM model_item_events x JOIN model_items i ON i.id = x.item_id
                     WHERE x.event_id = e.id AND i.session_id = ?3)
                 ORDER BY e.sequence"
            )?;
            let rows = query.query_map(
                params![
                    persona.id,
                    cutoff,
                    session_id,
                    event_start,
                    pending.receipt_cutoff,
                    pending.retry_of.is_some(),
                    user_id
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )?;
            for row in rows {
                let (id, sequence, conversation, message, kind, speaker, text) = row?;
                events.push(
                    json!({"event_id":id, "sequence":sequence, "conversation_id":conversation,
                    "message_id":message, "speaker":{"kind":kind,"id":speaker}, "text":text}),
                );
                event_ids.push(id);
            }
        }
        let call_id = pending.call_id.clone();
        let input_sequence: i64 = tx.query_row(
            "SELECT COALESCE(MAX(sequence), -1) + 1 FROM model_items WHERE session_id = ?1",
            [&session_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "UPDATE model_calls SET session_id = ?2, input_last_sequence = ?3, event_cutoff = ?4, status = 'streaming' WHERE id = ?1 AND status = 'queued'",
            params![call_id, session_id, input_sequence, cutoff],
        )?;
        let envelope = json!({"events":events, "reply_request":{
            "conversation_id":persona.conversation_id, "trigger_message_id":user_id}});
        let item = insert_item(
            &tx,
            &session_id,
            Some(&call_id),
            "input",
            &input_message(&protocol, "user", &serde_json::to_string(&envelope)?),
        )?;
        for event in event_ids {
            link(&tx, &item, &event)?;
        }
        update_frontier(&tx, &session_id, &persona.id)?;
        let turn = Turn {
            call_id,
            session_id,
            receipt_cutoff: pending.receipt_cutoff,
            persona_id: persona.id.clone(),
            conversation_id: persona.conversation_id.clone(),
            user: message(&tx, &user_id)?,
            reply: message(&tx, &reply_id)?,
            retry_of: pending.retry_of.clone(),
        };
        tx.commit()?;
        Ok(turn)
    }

    pub(crate) fn checkpoint(&self, turn: &Turn, content: &str) -> Result<(), Error> {
        self.connection.lock().expect("database lock").execute(
            "UPDATE messages SET content = ?2 WHERE id = ?1 AND status = 'streaming'",
            params![turn.reply.id, content],
        )?;
        Ok(())
    }

    pub(crate) fn finish_turn(
        &self,
        turn: &Turn,
        content: &str,
        status: MessageStatus,
        error: Option<&str>,
        native: Option<&[Value]>,
    ) -> Result<Message, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE messages SET content = ?2, status = ?3 WHERE id = ?1 AND status = 'streaming'",
            params![turn.reply.id, content, status.as_str()],
        )?;
        if changed > 0 {
            if status == MessageStatus::Complete {
                let fallback = [json!({"role":"assistant", "content":content})];
                let outputs = native.unwrap_or(&fallback);
                let event = insert_event_for(
                    &tx,
                    &turn.persona_id,
                    &turn.conversation_id,
                    &turn.reply.id,
                    "sent",
                )?;
                for payload in outputs {
                    let output = insert_item(
                        &tx,
                        &turn.session_id,
                        Some(&turn.call_id),
                        "output",
                        payload,
                    )?;
                    link(&tx, &output, &event)?;
                }
                update_frontier(&tx, &turn.session_id, &turn.persona_id)?;
            }
            let call_status = if status == MessageStatus::Complete {
                "completed"
            } else {
                status.as_str()
            };
            tx.execute(
                "UPDATE model_calls SET status = ?2, error = ?3,
                 finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![turn.call_id, call_status, error],
            )?;
        }
        let message = message(&tx, &turn.reply.id)?;
        tx.commit()?;
        Ok(message)
    }
}

pub(super) fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub(super) fn message(db: &Connection, id: &str) -> Result<Message, Error> {
    let mut message=db.query_row("SELECT id, conversation_id, sequence, sender_kind, sender_id, content, status, created_at FROM messages WHERE id = ?1", [id],read_message)?;
    let metadata:Option<(String,bool)>=db.query_row("SELECT COALESCE(c.error,''), (c.status IN ('failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM model_calls newer JOIN messages n ON n.id=newer.reply_message_id WHERE newer.conversation_id=c.conversation_id AND n.sequence>(SELECT sequence FROM messages WHERE id=?1)) AND NOT EXISTS(SELECT 1 FROM model_calls busy WHERE busy.conversation_id=c.conversation_id AND busy.status IN ('queued','streaming'))) FROM model_calls c WHERE c.reply_message_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((detail, retry)) = metadata {
        message.failure_detail = detail;
        message.can_retry = retry;
    }
    Ok(message)
}

pub(super) fn insert_message(
    db: &Connection,
    persona: &Persona,
    kind: &str,
    sender: &str,
    text: &str,
    status: MessageStatus,
) -> Result<String, Error> {
    let id = new_id();
    db.execute(
        "INSERT INTO messages (id, conversation_id, sequence, sender_kind, sender_id, content, status)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE conversation_id = ?2), ?3, ?4, ?5, ?6)",
        params![id, persona.conversation_id, kind, sender, text, status.as_str()],
    )?;
    Ok(id)
}

pub(super) fn insert_event(
    db: &Connection,
    persona: &Persona,
    message: &str,
    kind: &str,
) -> Result<String, Error> {
    insert_event_for(db, &persona.id, &persona.conversation_id, message, kind)
}

fn insert_event_for(
    db: &Connection,
    persona: &str,
    conversation: &str,
    message: &str,
    kind: &str,
) -> Result<String, Error> {
    let id = new_id();
    db.execute(
        "INSERT INTO persona_events (id, persona_id, sequence, conversation_id, message_id, kind)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(sequence), 0) + 1 FROM persona_events WHERE persona_id = ?2), ?3, ?4, ?5)",
        params![id, persona, conversation, message, kind],
    )?;
    Ok(id)
}

pub(super) fn insert_item(
    db: &Connection,
    session: &str,
    call: Option<&str>,
    kind: &str,
    payload: &Value,
) -> Result<String, Error> {
    let id = new_id();
    let (codec, bytes, length) = super::codec::encode(serde_json::to_vec(payload)?)?;
    db.execute(
        "INSERT INTO model_items (id, session_id, sequence, call_id, kind, codec, payload, decoded_length)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(sequence), -1) + 1 FROM model_items WHERE session_id = ?2), ?3, ?4, ?5, ?6, ?7)",
        params![id, session, call, kind, codec, bytes, length as i64],
    )?;
    Ok(id)
}

fn link(db: &Connection, item: &str, event: &str) -> Result<(), Error> {
    db.execute(
        "INSERT INTO model_item_events (item_id, event_id) VALUES (?1, ?2)",
        params![item, event],
    )?;
    Ok(())
}

fn update_frontier(db: &Connection, session: &str, persona: &str) -> Result<(), Error> {
    db.execute(
        "UPDATE model_sessions SET covered_event_sequence = COALESCE(
            (SELECT MIN(e.sequence) - 1 FROM persona_events e WHERE e.persona_id = ?2 AND e.kind = 'received' AND e.sequence > (SELECT event_start_sequence FROM model_sessions WHERE id = ?1) AND NOT EXISTS (SELECT 1 FROM model_calls c WHERE c.trigger_message_id = e.message_id AND c.session_id IS NULL AND c.status IN ('cancelled', 'interrupted', 'failed') AND NOT EXISTS (SELECT 1 FROM model_calls live WHERE live.trigger_message_id=e.message_id AND (live.session_id IS NOT NULL OR live.status IN ('queued','streaming')))) AND NOT EXISTS (
                SELECT 1 FROM model_item_events x JOIN model_items i ON i.id = x.item_id
                WHERE x.event_id = e.id AND i.session_id = ?1)),
            (SELECT COALESCE(MAX(sequence), 0) FROM persona_events WHERE persona_id = ?2)) WHERE id = ?1",
        params![session, persona],
    )?;
    Ok(())
}

pub(super) fn input_message(protocol: &str, role: &str, content: &str) -> Value {
    if protocol == "responses" {
        json!({"type":"message", "role":role, "content":[{"type":"input_text", "text":content}]})
    } else {
        json!({"role":role, "content":content})
    }
}
