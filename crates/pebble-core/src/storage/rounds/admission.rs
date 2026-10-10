use super::super::turns::{insert_message, message};
use super::*;
use crate::MessageStatus;

pub(super) fn admit(db: &Connection, slot: &str) -> Result<Step, Error> {
    let (decision, round, kind): (String, String, String) = db.query_row(
        "SELECT decision_id,round_id,kind FROM group_reply_slots WHERE id=?1 AND status='pending'",
        [slot],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let d = read(db, &decision)?;
    let eligible:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM conversation_memberships WHERE id=?1 AND left_publication IS NULL) AND EXISTS(SELECT 1 FROM group_rounds WHERE id=?2 AND (status='active' OR (status='superseded' AND ?3='required')))",params![d.membership_id,round,kind],|r|r.get(0))?;
    if !eligible {
        db.execute(
            "UPDATE group_reply_slots SET status='cancelled' WHERE id=?1",
            [slot],
        )?;
        db.execute("UPDATE participation_decisions SET status='cancelled',error='The original room membership or request is no longer active.' WHERE id=?1",[decision])?;
        return Ok(Step::Continue);
    }
    if kind == "followup" {
        if db.execute("UPDATE group_rounds SET followups_used=followups_used+1 WHERE id=?1 AND followups_used<2 AND status='active' AND continuation=1",[&round])?==0 {
            db.execute("UPDATE group_reply_slots SET status='cancelled' WHERE id=?1", [slot])?;
            return Ok(Step::Continue);
        }
        // A wave admits at most one speaker; other decisions cannot later create another call.
        db.execute("UPDATE participation_decisions SET status='cancelled',error='' WHERE id IN (SELECT decision_id FROM group_reply_slots WHERE round_id=?1 AND kind='followup' AND status='pending' AND id!=?2) AND status IN ('pending','selected')",params![round,slot])?;
        db.execute("UPDATE group_reply_slots SET status='declined' WHERE round_id=?1 AND kind='followup' AND status='pending' AND id!=?2",params![round,slot])?;
    }
    let reply = insert_message(
        db,
        &d.conversation_id,
        "persona",
        &d.persona_id,
        "",
        MessageStatus::Streaming,
    )?;
    let call = new_id();
    db.execute("INSERT INTO model_calls(id,conversation_id,trigger_message_id,reply_message_id,event_cutoff,status,persona_id,persona_sequence,membership_id,slot_id) VALUES (?1,?2,?3,?4,?5,'queued',?6,(SELECT COALESCE(MAX(persona_sequence),0)+1 FROM model_calls WHERE persona_id=?6),?7,?8)",params![call,d.conversation_id,d.trigger_message_id,reply,d.event_cutoff,d.persona_id,d.membership_id,slot])?;
    db.execute(
        "UPDATE group_reply_slots SET status='admitted' WHERE id=?1",
        [slot],
    )?;
    // Keep mentioned as an explicit-user status; automatic selections remain selected.
    db.execute("UPDATE participation_decisions SET status=CASE WHEN ?3='required' THEN 'mentioned' ELSE 'selected' END,call_id=?2 WHERE id=?1",params![decision,call,kind])?;
    let mut reply = message(db, &reply)?;
    reply.status = MessageStatus::Queued;
    Ok(Step::Admit(
        Box::new(Turn {
            call_id: call,
            session_id: String::new(),
            receipt_cutoff: d.event_cutoff,
            persona_id: d.persona_id,
            conversation_id: d.conversation_id,
            user: message(db, &d.trigger_message_id)?,
            reply,
            retry_of: None,
        }),
        decision,
    ))
}

pub(super) fn reset(
    db: &Connection,
    id: &str,
    round: &str,
    persona: &Persona,
    phase: &str,
) -> Result<DecisionWork, Error> {
    db.execute("UPDATE participation_decisions SET generation=generation+1,phase=?2,status='pending',respond=NULL,error='',elapsed_ms=NULL,knowledge_version=?3,predecessor_version=?4 WHERE id=?1",params![id,phase,knowledge(db,&persona.id)?,predecessors(db,round)?])?;
    work(db, id, persona)
}
