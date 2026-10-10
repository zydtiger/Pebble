use super::*;

fn finish(db: &Connection, round: &str) -> Result<Step, Error> {
    db.execute(
        "UPDATE group_reply_slots SET status='declined' WHERE round_id=?1 AND status='pending'",
        [round],
    )?;
    db.execute("UPDATE group_rounds SET status='completed',continuation=0 WHERE id=?1 AND status IN ('active','superseded')",[round])?;
    Ok(Step::Finished)
}

pub(super) fn step(db: &Connection, round: &str, personas: &[Persona]) -> Result<Step, Error> {
    let (room,status,used,last,trigger,stage):(String,String,i64,Option<String>,Option<String>,String)=db.query_row("SELECT conversation_id,status,followups_used,last_reply,followup_trigger,fallback_stage FROM group_rounds WHERE id=?1",[round],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
    if !matches!(status.as_str(), "active" | "superseded") {
        return Ok(Step::Finished);
    }
    // Only the room's ready head can join a persona queue. Older protected mentions drain first.
    let blocked:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM group_reply_slots s JOIN group_rounds r ON r.id=s.round_id WHERE r.conversation_id=?1 AND s.status IN ('pending','admitted') AND (s.status='admitted' OR r.revision<(SELECT revision FROM group_rounds WHERE id=?2)))",params![room,round],|r|r.get(0))?;
    if blocked {
        return Ok(Step::Wait);
    }
    loop {
        let head:Option<(String,String,String)>=db.query_row("SELECT id,decision_id,kind FROM group_reply_slots WHERE round_id=?1 AND status='pending' ORDER BY ordinal LIMIT 1",[round],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let Some((slot, id, kind)) = head else {
            break;
        };
        let d = read(db, &id)?;
        if kind == "followup"
            && !db.query_row(
                "SELECT continuation FROM group_rounds WHERE id=?1",
                [round],
                |r| r.get::<_, bool>(0),
            )?
        {
            db.execute("UPDATE group_reply_slots SET status='cancelled' WHERE round_id=?1 AND kind='followup' AND status='pending'",[round])?;
            db.execute("UPDATE participation_decisions SET status='cancelled',error='Automatic continuation ended.' WHERE id IN (SELECT decision_id FROM group_reply_slots WHERE round_id=?1 AND kind='followup' AND status='cancelled') AND status IN ('pending','selected')",[round])?;
            break;
        }
        if kind == "required" {
            return admission::admit(db, &slot);
        }
        if d.status == "pending" {
            return Ok(Step::Wait);
        }
        let persona = personas
            .iter()
            .find(|p| p.id == d.persona_id)
            .ok_or(Error::UnknownPersona)?;
        let (phase,known,pred):(String,i64,i64)=db.query_row("SELECT phase,knowledge_version,predecessor_version FROM participation_decisions WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if kind == "optional"
            && matches!(d.status.as_str(), "selected" | "listening")
            && (known != knowledge(db, &persona.id)? || pred != predecessors(db, round)?)
        {
            return Ok(Step::Classify(vec![admission::reset(
                db,
                &id,
                round,
                persona,
                "participation",
            )?]));
        }
        if kind == "fallback" && phase == "intent" {
            if d.respond != Some(true) {
                return finish(db, round);
            }
            db.execute(
                "UPDATE group_rounds SET fallback_stage='permission' WHERE id=?1",
                [round],
            )?;
            return Ok(Step::Classify(vec![admission::reset(
                db,
                &id,
                round,
                persona,
                "permission",
            )?]));
        }
        if d.status == "selected" {
            return admission::admit(db, &slot);
        }
        db.execute(
            "UPDATE group_reply_slots SET status='declined' WHERE id=?1",
            [slot],
        )?;
        if matches!(d.status.as_str(), "failed" | "cancelled" | "interrupted") {
            db.execute(
                "UPDATE group_rounds SET continuation=0 WHERE id=?1",
                [round],
            )?;
            if kind == "fallback" {
                return finish(db, round);
            }
        }
    }
    if status == "superseded" {
        return finish(db, round);
    }
    if stage == "permission" {
        let next:Option<String>=db.query_row("SELECT d.id FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE s.round_id=?1 AND s.status='declined' AND d.phase='participation' AND d.respond=0 ORDER BY s.ordinal LIMIT 1",[round],|r|r.get(0)).optional()?;
        if let Some(id) = next {
            let d = read(db, &id)?;
            let persona = personas
                .iter()
                .find(|p| p.id == d.persona_id)
                .ok_or(Error::UnknownPersona)?;
            db.execute("UPDATE group_reply_slots SET kind='fallback',status='pending' WHERE decision_id=?1",[&id])?;
            return Ok(Step::Classify(vec![admission::reset(
                db,
                &id,
                round,
                persona,
                "permission",
            )?]));
        }
        return finish(db, round);
    }
    // Re-read after initial errors may have disabled automatic continuation above.
    let continuation: bool = db.query_row(
        "SELECT continuation FROM group_rounds WHERE id=?1",
        [round],
        |r| r.get(0),
    )?;
    if !continuation {
        return finish(db, round);
    }
    if let Some(last) = last {
        if used >= 2 || trigger.as_ref() == Some(&last) {
            return finish(db, round);
        }
        return followup(db, round, &room, &last, personas);
    }
    if stage != "none" {
        return finish(db, round);
    }
    let all_false:bool=db.query_row("SELECT NOT EXISTS(SELECT 1 FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE s.round_id=?1 AND (d.status!='listening' OR d.respond IS NOT 0))",[round],|r|r.get(0))?;
    if !all_false {
        return finish(db, round);
    }
    let first: Option<String> = db
        .query_row(
            "SELECT decision_id FROM group_reply_slots WHERE round_id=?1 ORDER BY ordinal LIMIT 1",
            [round],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = first {
        let d = read(db, &id)?;
        let persona = personas
            .iter()
            .find(|p| p.id == d.persona_id)
            .ok_or(Error::UnknownPersona)?;
        if persona.classifier.is_none() {
            return finish(db, round);
        }
        db.execute(
            "UPDATE group_rounds SET fallback_stage='intent' WHERE id=?1",
            [round],
        )?;
        db.execute(
            "UPDATE group_reply_slots SET kind='fallback',status='pending' WHERE decision_id=?1",
            [&id],
        )?;
        return Ok(Step::Classify(vec![admission::reset(
            db, &id, round, persona, "intent",
        )?]));
    }
    finish(db, round)
}

fn followup(
    db: &Connection,
    round: &str,
    room: &str,
    last: &str,
    personas: &[Persona],
) -> Result<Step, Error> {
    db.execute(
        "UPDATE group_rounds SET followup_trigger=?2 WHERE id=?1",
        params![round, last],
    )?;
    let candidates=db.prepare("SELECT d.persona_id,d.membership_id,d.event_cutoff,d.revision FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id JOIN conversation_memberships m ON m.id=d.membership_id WHERE s.round_id=?1 AND d.message_id=?1 AND m.left_publication IS NULL AND d.persona_id!=(SELECT sender_id FROM messages WHERE id=?2) ORDER BY COALESCE((SELECT MAX(publication_sequence) FROM messages WHERE conversation_id=?3 AND sender_id=d.persona_id AND sender_kind='persona' AND status='complete'),0),m.ordinal,d.persona_id")?.query_map(params![round,last,room],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?)))?.collect::<Result<Vec<_>,_>>()?;
    let mut ordinal: i64 = db.query_row(
        "SELECT COALESCE(MAX(ordinal),-1)+1 FROM group_reply_slots WHERE round_id=?1",
        [round],
        |r| r.get(0),
    )?;
    let mut pending = Vec::new();
    for (persona_id, membership, cutoff, revision) in candidates {
        let persona = personas
            .iter()
            .find(|p| p.id == persona_id)
            .ok_or(Error::UnknownPersona)?;
        if persona.classifier.is_none() {
            continue;
        }
        let id = new_id();
        db.execute("INSERT INTO participation_decisions(id,message_id,conversation_id,persona_id,membership_id,event_cutoff,revision,status,knowledge_version,predecessor_version) VALUES (?1,?2,?3,?4,?5,?6,?7,'pending',?8,?9)",params![id,last,room,persona_id,membership,cutoff,revision,knowledge(db,&persona_id)?,predecessors(db,round)?])?;
        db.execute("INSERT INTO group_reply_slots(id,round_id,decision_id,ordinal,kind) VALUES (?1,?2,?3,?4,'followup')",params![new_id(),round,id,ordinal])?;
        pending.push(work(db, &id, persona)?);
        ordinal += 1;
    }
    if pending.is_empty() {
        finish(db, round)
    } else {
        Ok(Step::Classify(pending))
    }
}
