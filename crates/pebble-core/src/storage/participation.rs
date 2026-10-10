use super::{
    Database,
    turns::{insert_message, message, new_id},
};
use crate::{Error, MessageStatus, Participation, Persona, history::Turn};
use rusqlite::{Connection, params};

pub(super) fn read(db: &Connection, id: &str) -> Result<Participation, Error> {
    Ok(db.query_row("SELECT id,message_id,conversation_id,persona_id,membership_id,event_cutoff,status,error,call_id,elapsed_ms,revision,respond FROM participation_decisions WHERE id=?1", [id], |r|Ok(Participation {id:r.get(0)?,message_id:r.get(1)?,conversation_id:r.get(2)?,persona_id:r.get(3)?,membership_id:r.get(4)?,event_cutoff:r.get(5)?,status:r.get(6)?,error:r.get(7)?,call_id:r.get(8)?,elapsed_ms:r.get::<_,Option<i64>>(9)?.map(|v|v as u64),revision:r.get(10)?,respond:r.get(11)?}))?)
}
impl Database {
    pub(crate) fn participation(&self, room: &str) -> Result<Vec<Participation>, Error> {
        let db = self.connection.lock().expect("database lock");
        super::rooms::room(&db, room)?;
        let mut q=db.prepare("SELECT d.id FROM participation_decisions d JOIN messages m ON m.id=d.message_id WHERE d.conversation_id=?1 ORDER BY m.sequence DESC,d.rowid")?;
        q.query_map([room], |r| r.get::<_, String>(0))?
            .map(|id| read(&db, &id?))
            .collect()
    }
    pub(crate) fn decision(&self, id: &str) -> Result<Participation, Error> {
        read(&self.connection.lock().expect("database lock"), id)
    }
    pub(crate) fn finish_decision(
        &self,
        id: &str,
        status: &str,
        error: &str,
        respond: Option<bool>,
        elapsed_ms: Option<u64>,
    ) -> Result<Participation, Error> {
        let db = self.connection.lock().expect("database lock");
        db.execute("UPDATE participation_decisions SET status=?2,error=?3,respond=?4,elapsed_ms=?5 WHERE id=?1 AND status='pending'",params![id,status,crate::participation::clip(error,600),respond,elapsed_ms.map(|v|v.min(i64::MAX as u64) as i64)])?;
        read(&db, id)
    }
    pub(crate) fn ensure_decisions_idle(&self, persona: Option<&str>) -> Result<(), Error> {
        let db = self.connection.lock().expect("database lock");
        if db.query_row("SELECT EXISTS(SELECT 1 FROM participation_decisions WHERE status='pending' AND (?1 IS NULL OR persona_id=?1))",[persona],|r|r.get::<_,bool>(0))? {return Err(Error::Busy);}
        Ok(())
    }
    /// Selection and a single generation admission share one SQLite transaction.
    pub(crate) fn admit_decision(
        &self,
        id: &str,
        persona: &Persona,
        elapsed_ms: u64,
    ) -> Result<Option<Turn>, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let d = read(&tx, id)?;
        if d.status != "pending" {
            return Ok(None);
        }
        let valid=tx.query_row("SELECT EXISTS(SELECT 1 FROM conversation_memberships i WHERE i.id=?1 AND i.left_publication IS NULL) AND NOT EXISTS(SELECT 1 FROM messages m WHERE m.conversation_id=?2 AND m.sender_kind='user' AND m.sequence>?3)",params![d.membership_id,d.conversation_id,d.revision],|r|r.get::<_,bool>(0))?;
        if !valid {
            tx.execute("UPDATE participation_decisions SET status='cancelled',error='A newer message or membership change replaced this decision.' WHERE id=?1",[id])?;
            tx.commit()?;
            return Ok(None);
        }
        let reply = insert_message(
            &tx,
            &d.conversation_id,
            "persona",
            &persona.id,
            "",
            MessageStatus::Streaming,
        )?;
        let call_id = new_id();
        tx.execute("INSERT INTO model_calls (id,conversation_id,trigger_message_id,reply_message_id,event_cutoff,status,persona_id,persona_sequence,membership_id) VALUES (?1,?2,?3,?4,?5,'queued',?6,(SELECT COALESCE(MAX(persona_sequence),0)+1 FROM model_calls WHERE persona_id=?6),?7)",params![call_id,d.conversation_id,d.message_id,reply,d.event_cutoff,persona.id,d.membership_id])?;
        tx.execute("UPDATE participation_decisions SET status='selected',respond=1,call_id=?2,elapsed_ms=?3 WHERE id=?1 AND status='pending'",params![id,call_id,elapsed_ms.min(i64::MAX as u64) as i64])?;
        let mut reply = message(&tx, &reply)?;
        reply.status = MessageStatus::Queued;
        let turn = Turn {
            call_id,
            session_id: String::new(),
            receipt_cutoff: d.event_cutoff,
            persona_id: persona.id.clone(),
            conversation_id: d.conversation_id,
            user: message(&tx, &d.message_id)?,
            reply,
            retry_of: None,
        };
        tx.commit()?;
        Ok(Some(turn))
    }
}
