use super::{
    Database,
    turns::{insert_event, insert_message, message, new_id},
};
use crate::{Error, Message, MessageStatus, Persona, history::Turn};
use rusqlite::params;

impl Database {
    /// Acceptance saves visible messages and receipt; model history freezes at execution.
    pub(crate) fn enqueue_turn(&self, persona: &Persona, prompt: &str) -> Result<Turn, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        let user = insert_message(
            &tx,
            persona,
            "user",
            "owner",
            prompt,
            MessageStatus::Complete,
        )?;
        let reply = insert_message(
            &tx,
            persona,
            "persona",
            &persona.id,
            "",
            MessageStatus::Streaming,
        )?;
        insert_event(&tx, persona, &user, "received")?;
        let receipt_cutoff = tx.query_row(
            "SELECT MAX(sequence) FROM persona_events WHERE persona_id = ?1",
            [&persona.id],
            |row| row.get::<_, i64>(0),
        )?;
        let call_id = new_id();
        tx.execute("INSERT INTO model_calls (id, conversation_id, trigger_message_id, reply_message_id, event_cutoff, status)
            VALUES (?1, ?2, ?3, ?4, ?5, 'queued')", params![call_id, persona.conversation_id, user, reply, receipt_cutoff])?;
        let mut reply = message(&tx, &reply)?;
        reply.status = MessageStatus::Queued;
        let pending = Turn {
            call_id,
            session_id: String::new(),
            receipt_cutoff,
            persona_id: persona.id.clone(),
            conversation_id: persona.conversation_id.clone(),
            user: message(&tx, &user)?,
            reply,
            retry_of: None,
        };
        tx.commit()?;
        Ok(pending)
    }

    pub(crate) fn finish_pending(
        &self,
        pending: &Turn,
        status: MessageStatus,
        error: Option<&str>,
    ) -> Result<Message, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        tx.execute(
            "UPDATE messages SET status = ?2 WHERE id = ?1 AND status = 'streaming'",
            params![pending.reply.id, status.as_str()],
        )?;
        tx.execute("UPDATE model_calls SET status = ?2, error = ?3, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND status = 'queued'", params![pending.call_id, status.as_str(), error])?;
        let reply = message(&tx, &pending.reply.id)?;
        tx.commit()?;
        Ok(reply)
    }
}
