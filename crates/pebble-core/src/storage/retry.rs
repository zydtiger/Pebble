use super::{
    Database,
    turns::{insert_message, message, new_id},
};
use crate::{Error, MessageStatus, Persona, history::Turn};
use rusqlite::{OptionalExtension, params};
impl Database {
    pub(crate) fn ensure_idle(&self, persona: Option<&str>) -> Result<(), Error> {
        let connection = self.connection.lock().expect("database lock");
        if connection.query_row("SELECT EXISTS(SELECT 1 FROM model_calls c WHERE c.status IN ('queued','streaming') AND (?1 IS NULL OR c.persona_id=?1)) OR EXISTS(SELECT 1 FROM context_maintenance WHERE status='running' AND (?1 IS NULL OR persona_id=?1)) OR EXISTS(SELECT 1 FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE s.status IN ('pending','admitted') AND (?1 IS NULL OR d.persona_id=?1))",[persona],|r|r.get::<_,bool>(0))? {return Err(Error::Busy);}
        Ok(())
    }
    pub(crate) fn enqueue_retry(&self, persona: &Persona, reply: &str) -> Result<Turn, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM model_calls WHERE persona_id=?1 AND status IN ('queued','streaming')) OR EXISTS(SELECT 1 FROM context_maintenance WHERE persona_id=?1 AND status='running') OR EXISTS(SELECT 1 FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE d.persona_id=?1 AND s.status IN ('pending','admitted'))",[&persona.id],|r|r.get::<_,bool>(0))? {return Err(Error::Busy);}
        let latest:Option<(String,String,String,String,String,Option<String>)>=tx.query_row("SELECT c.id,c.trigger_message_id,c.status,c.reply_message_id,c.conversation_id,c.membership_id FROM model_calls c WHERE c.persona_id=?1 ORDER BY c.persona_sequence DESC LIMIT 1",[&persona.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
        let Some((original, user, status, latest_reply, conversation, membership)) = latest else {
            return Err(unavailable());
        };
        if latest_reply != reply
            || !matches!(status.as_str(), "failed" | "cancelled" | "interrupted")
        {
            return Err(unavailable());
        }
        if let Some(id) = &membership
            && !tx.query_row("SELECT EXISTS(SELECT 1 FROM conversation_memberships WHERE id=?1 AND persona_id=?2 AND conversation_id=?3 AND left_publication IS NULL)",params![id,persona.id,conversation],|r|r.get::<_,bool>(0))? {return Err(unavailable());}
        let receipt = tx.query_row(
            "SELECT sequence FROM persona_events WHERE message_id=?1 AND persona_id=?2 AND kind='received'",
            params![user,persona.id],
            |r| r.get::<_, i64>(0),
        )?;
        let reply = insert_message(
            &tx,
            &conversation,
            "persona",
            &persona.id,
            "",
            MessageStatus::Streaming,
        )?;
        let call = new_id();
        tx.execute("INSERT INTO model_calls (id,conversation_id,trigger_message_id,reply_message_id,event_cutoff,status,retry_of,persona_id,persona_sequence,membership_id) VALUES (?1,?2,?3,?4,?5,'queued',?6,?7,(SELECT COALESCE(MAX(persona_sequence),0)+1 FROM model_calls WHERE persona_id=?7),?8)",params![call,conversation,user,reply,receipt,original,persona.id,membership])?;
        let mut reply = message(&tx, &reply)?;
        reply.status = MessageStatus::Queued;
        let turn = Turn {
            call_id: call,
            session_id: String::new(),
            receipt_cutoff: receipt,
            persona_id: persona.id.clone(),
            conversation_id: conversation,
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
