use super::Database;
use crate::{Error, Participation};
use rusqlite::{Connection, params};

pub(super) fn read(db: &Connection, id: &str) -> Result<Participation, Error> {
    Ok(db.query_row("SELECT d.id,COALESCE(s.round_id,d.message_id),d.conversation_id,d.persona_id,d.membership_id,d.event_cutoff,d.status,d.error,d.call_id,d.elapsed_ms,d.revision,d.respond,d.message_id,COALESCE(s.status,''),d.phase FROM participation_decisions d LEFT JOIN group_reply_slots s ON s.decision_id=d.id WHERE d.id=?1", [id], |r|Ok(Participation {id:r.get(0)?,message_id:r.get(1)?,conversation_id:r.get(2)?,persona_id:r.get(3)?,membership_id:r.get(4)?,event_cutoff:r.get(5)?,status:r.get(6)?,error:r.get(7)?,call_id:r.get(8)?,elapsed_ms:r.get::<_,Option<i64>>(9)?.map(|v|v as u64),revision:r.get(10)?,respond:r.get(11)?,trigger_message_id:r.get(12)?,slot_status:r.get(13)?,phase:r.get(14)?}))?)
}
impl Database {
    pub(crate) fn participation(&self, room: &str) -> Result<Vec<Participation>, Error> {
        let db = self.connection.lock().expect("database lock");
        super::rooms::room(&db, room)?;
        let mut q=db.prepare("SELECT d.id FROM participation_decisions d JOIN messages m ON m.id=d.message_id WHERE d.conversation_id=?1 ORDER BY d.revision,d.rowid")?;
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
        generation: i64,
        status: &str,
        error: &str,
        respond: Option<bool>,
        elapsed_ms: Option<u64>,
    ) -> Result<Participation, Error> {
        let db = self.connection.lock().expect("database lock");
        db.execute("UPDATE participation_decisions SET status=?2,error=?3,respond=?4,elapsed_ms=?5 WHERE id=?1 AND status='pending' AND generation=?6",params![id,status,crate::participation::clip(error,600),respond,elapsed_ms.map(|v|v.min(i64::MAX as u64) as i64),generation])?;
        read(&db, id)
    }
    pub(crate) fn cancel_decision(&self, id: &str) -> Result<Participation, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        tx.execute("UPDATE participation_decisions SET status='cancelled',error='Participation stopped; mention this character to continue.' WHERE id=?1 AND call_id IS NULL AND status IN ('pending','selected','mentioned')",[id])?;
        tx.execute("UPDATE group_reply_slots SET status='cancelled' WHERE decision_id=?1 AND status='pending'",[id])?;
        tx.execute("UPDATE group_rounds SET continuation=0 WHERE id IN (SELECT round_id FROM group_reply_slots WHERE decision_id=?1)",[id])?;
        let d = read(&tx, id)?;
        tx.commit()?;
        Ok(d)
    }
    pub(crate) fn cancel_reassessment(
        &self,
        id: &str,
        call: &str,
        generation: i64,
    ) -> Result<(), Error> {
        let db = self.connection.lock().expect("database lock");
        db.execute(
            "UPDATE participation_decisions SET status='cancelled',error='This reply was stopped.',respond=NULL WHERE id=?1 AND call_id=?2 AND generation=?3 AND status='pending'",
            params![id, call, generation],
        )?;
        Ok(())
    }
    pub(crate) fn ensure_decisions_idle(&self, persona: Option<&str>) -> Result<(), Error> {
        let db = self.connection.lock().expect("database lock");
        if db.query_row("SELECT EXISTS(SELECT 1 FROM participation_decisions WHERE status='pending' AND (?1 IS NULL OR persona_id=?1)) OR EXISTS(SELECT 1 FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE s.status IN ('pending','admitted') AND (?1 IS NULL OR d.persona_id=?1))",[persona],|r|r.get::<_,bool>(0))? {return Err(Error::Busy);}
        Ok(())
    }
}
