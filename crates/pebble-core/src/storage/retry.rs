use super::{
    Database,
    turns::{insert_message, message, new_id},
};
use crate::{Error, MessageStatus, Persona, history::Turn};
use rusqlite::{OptionalExtension, params};
impl Database {
    pub(crate) fn ensure_idle(&self, persona: Option<&str>) -> Result<(), Error> {
        let connection = self.connection.lock().expect("database lock");
        if connection.query_row("SELECT EXISTS(SELECT 1 FROM model_calls c JOIN conversations r ON r.id=c.conversation_id WHERE c.status IN ('queued','streaming') AND (?1 IS NULL OR r.persona_id=?1))",[persona],|r|r.get::<_,bool>(0))? {return Err(Error::Busy);}
        Ok(())
    }
    pub(crate) fn enqueue_retry(&self, persona: &Persona, reply: &str) -> Result<Turn, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM model_calls WHERE conversation_id=?1 AND status IN ('queued','streaming'))",[&persona.conversation_id],|r|r.get::<_,bool>(0))? {return Err(Error::Busy);}
        let latest:Option<(String,String,String,String)>=tx.query_row("SELECT c.id,c.trigger_message_id,c.status,c.reply_message_id FROM model_calls c JOIN messages m ON m.id=c.reply_message_id WHERE c.conversation_id=?1 ORDER BY m.sequence DESC LIMIT 1",[&persona.conversation_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let Some((original, user, status, latest_reply)) = latest else {
            return Err(unavailable());
        };
        if latest_reply != reply
            || !matches!(status.as_str(), "failed" | "cancelled" | "interrupted")
        {
            return Err(unavailable());
        }
        let receipt = tx.query_row(
            "SELECT sequence FROM persona_events WHERE message_id=?1 AND kind='received'",
            [&user],
            |r| r.get::<_, i64>(0),
        )?;
        let reply = insert_message(
            &tx,
            persona,
            "persona",
            &persona.id,
            "",
            MessageStatus::Streaming,
        )?;
        let call = new_id();
        tx.execute("INSERT INTO model_calls (id,conversation_id,trigger_message_id,reply_message_id,event_cutoff,status,retry_of) VALUES (?1,?2,?3,?4,?5,'queued',?6)",params![call,persona.conversation_id,user,reply,receipt,original])?;
        let mut reply = message(&tx, &reply)?;
        reply.status = MessageStatus::Queued;
        let turn = Turn {
            call_id: call,
            session_id: String::new(),
            receipt_cutoff: receipt,
            persona_id: persona.id.clone(),
            conversation_id: persona.conversation_id.clone(),
            user: message(&tx, &user)?,
            reply,
            retry_of: Some(original),
        };
        tx.commit()?;
        Ok(turn)
    }
}
fn unavailable() -> Error {
    Error::RetryUnavailable("Only this character's latest failed, stopped, or interrupted reply can be retried. Earlier attempts stay saved.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unactivated_cancelled_retry_reuses_nearest_frozen_ancestor() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::Core::open(root.path()).unwrap();
        let persona = core.workspace().unwrap().personas.remove(0);
        let pending = core
            .database
            .enqueue_turn(&persona, "original user")
            .unwrap();
        let active = core.database.activate_turn(&persona, &pending).unwrap();
        core.database
            .finish_turn(
                &active,
                "partial",
                MessageStatus::Failed,
                Some("synthetic failure"),
                None,
            )
            .unwrap();
        let original = core.database.request(&active.call_id).unwrap();
        let queued = core
            .database
            .enqueue_retry(&persona, &active.reply.id)
            .unwrap();
        core.database
            .finish_pending(&queued, MessageStatus::Cancelled, None)
            .unwrap();
        let again = core
            .database
            .enqueue_retry(&persona, &queued.reply.id)
            .unwrap();
        let active = core.database.activate_turn(&persona, &again).unwrap();
        let replay = core.database.request(&active.call_id).unwrap();
        assert_eq!(replay.items.len(), original.items.len());
        for (a, b) in replay.items.iter().zip(original.items) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.payload, b.payload);
        }
    }
}
