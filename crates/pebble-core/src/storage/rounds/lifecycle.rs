use super::*;

pub(in crate::storage) fn cancel_call(db: &Connection, id: &str) -> Result<(), Error> {
    db.execute("UPDATE messages SET status='cancelled' WHERE id=(SELECT reply_message_id FROM model_calls WHERE id=?1) AND status='streaming'",[id])?;
    db.execute("UPDATE model_calls SET status='cancelled',finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1 AND status IN ('queued','streaming')",[id])?;
    super::super::memory::annotate_stopped_memory(db, id, None)?;
    Ok(())
}
pub(in crate::storage) fn cancel_membership(
    db: &Connection,
    membership: &str,
) -> Result<Vec<String>, Error> {
    let calls = db.prepare("SELECT id FROM model_calls WHERE membership_id=?1 AND status IN ('queued','streaming')")?
        .query_map([membership], |r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
    db.execute("UPDATE group_rounds SET continuation=0 WHERE id IN (SELECT s.round_id FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE d.membership_id=?1 AND s.status IN ('pending','admitted'))",[membership])?;
    db.execute("UPDATE participation_decisions SET status='cancelled',error='This character left the group.' WHERE membership_id=?1 AND status IN ('pending','selected','mentioned') AND id IN (SELECT decision_id FROM group_reply_slots WHERE status IN ('pending','admitted'))",[membership])?;
    db.execute("UPDATE group_reply_slots SET status='cancelled' WHERE status IN ('pending','admitted') AND decision_id IN (SELECT id FROM participation_decisions WHERE membership_id=?1)",[membership])?;
    for call in &calls {
        cancel_call(db, call)?;
    }
    Ok(calls)
}
fn cancel_slots(db: &Connection, round: &str, optional_only: bool) -> Result<(), Error> {
    let ids=db.prepare("SELECT s.id,d.id,d.call_id FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE s.round_id=?1 AND s.status IN ('pending','admitted') AND (NOT ?2 OR s.kind!='required')")?.query_map(params![round,optional_only],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?)))?.collect::<Result<Vec<_>,_>>()?;
    for (slot, decision, call) in ids {
        if let Some(call) = call {
            cancel_call(db, &call)?;
        }
        db.execute(
            "UPDATE group_reply_slots SET status='cancelled' WHERE id=?1",
            [slot],
        )?;
        db.execute("UPDATE participation_decisions SET status='cancelled',error='This reply was stopped or superseded.' WHERE id=?1 AND status IN ('pending','selected','mentioned')",[decision])?;
    }
    Ok(())
}
pub(in crate::storage) fn invalidate_optional(db: &Connection, room: &str) -> Result<(), Error> {
    let rounds = db
        .prepare("SELECT id FROM group_rounds WHERE conversation_id=?1 AND status='active'")?
        .query_map([room], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for round in rounds {
        db.execute(
            "UPDATE group_rounds SET status='superseded',continuation=0 WHERE id=?1",
            [&round],
        )?;
        cancel_slots(db, &round, true)?;
    }
    Ok(())
}
pub(in crate::storage) fn cancel_round(db: &Connection, round: &str) -> Result<(), Error> {
    cancel_slots(db, round, false)?;
    db.execute("UPDATE group_rounds SET status='cancelled',continuation=0 WHERE id=?1 AND status IN ('active','superseded')",[round])?;
    Ok(())
}
pub(in crate::storage) fn complete_slot(
    db: &Connection,
    call: &str,
    status: &str,
    reply: &str,
) -> Result<(), Error> {
    let slot:Option<(String,String,String)>=db.query_row("SELECT s.id,s.round_id,s.kind FROM group_reply_slots s JOIN model_calls c ON c.slot_id=s.id WHERE c.id=?1",[call],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((slot, round, kind)) = slot {
        if db.execute(
            "UPDATE group_reply_slots SET status=?2 WHERE id=?1 AND status='admitted'",
            params![slot, status],
        )? == 0
        {
            return Ok(());
        }
        if status == "completed" {
            db.execute(
                "UPDATE group_rounds SET last_reply=?2 WHERE id=?1",
                params![round, reply],
            )?;
        } else {
            db.execute(
                "UPDATE group_rounds SET continuation=0 WHERE id=?1",
                [&round],
            )?;
        }
        if kind == "fallback" {
            db.execute("UPDATE group_rounds SET status='completed',continuation=0,fallback_stage='done' WHERE id=?1 AND status='active'",[round])?;
        }
    }
    Ok(())
}
