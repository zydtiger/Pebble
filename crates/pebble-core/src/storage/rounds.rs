//! Durable room coordination; only the ready speaker receives a model call.
mod admission;
mod lifecycle;
mod scheduling;
use super::{Database, participation::read, turns::new_id};
use crate::{Error, Persona, Room, history::Turn, participation::DecisionWork};
pub(super) use lifecycle::{cancel_round, complete_slot, invalidate_optional};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) enum Step {
    Wait,
    Continue,
    Finished,
    Classify(Vec<DecisionWork>),
    Admit(Box<Turn>, String),
}
pub(super) fn cancel_publication(db: &Connection, call: &str, reply: &str) -> Result<(), Error> {
    lifecycle::cancel_call(db, call)?;
    complete_slot(db, call, "cancelled", reply)
}

pub(super) fn knowledge(db: &Connection, persona: &str) -> Result<i64, Error> {
    Ok(db.query_row("SELECT COALESCE(MAX(persona_sequence),0) FROM model_calls WHERE persona_id=?1 AND status='completed'",[persona],|r|r.get(0))?)
}
pub(super) fn predecessors(db: &Connection, round: &str) -> Result<i64, Error> {
    Ok(db.query_row(
        "SELECT COUNT(*) FROM group_reply_slots WHERE round_id=?1 AND status='completed'",
        [round],
        |r| r.get(0),
    )?)
}
pub(super) fn work(db: &Connection, id: &str, persona: &Persona) -> Result<DecisionWork, Error> {
    let decision = read(db, id)?;
    let (generation, phase, round): (i64,String,String) = db.query_row("SELECT d.generation,d.phase,s.round_id FROM participation_decisions d JOIN group_reply_slots s ON s.decision_id=d.id WHERE d.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let input = crate::participation::round_input(
        db,
        persona,
        &decision.conversation_id,
        decision.event_cutoff,
        &round,
        &phase,
    )?;
    Ok(DecisionWork {
        decision,
        input,
        generation,
        phase,
        persona: persona.clone(),
    })
}

pub(super) fn accept(
    db: &Connection,
    room: &Room,
    user: &str,
    selected: &[String],
    personas: &[Persona],
) -> Result<Vec<DecisionWork>, Error> {
    invalidate_optional(db, &room.id)?;
    let revision: i64 = db.query_row("SELECT sequence FROM messages WHERE id=?1", [user], |r| {
        r.get(0)
    })?;
    db.execute(
        "INSERT INTO group_rounds(id,conversation_id,revision) VALUES (?1,?2,?3)",
        params![user, room.id, revision],
    )?;
    let mut members = Vec::new();
    for (ordinal, member) in room.members.iter().enumerate() {
        let last: i64 = db.query_row("SELECT COALESCE(MAX(publication_sequence),0) FROM messages WHERE conversation_id=?1 AND sender_id=?2 AND sender_kind='persona' AND status='complete'",params![room.id,member.persona_id],|r|r.get(0))?;
        let required = selected.iter().position(|id| id == &member.persona_id);
        members.push((
            (
                required.is_none(),
                required.unwrap_or(0),
                last,
                ordinal,
                member.persona_id.clone(),
            ),
            member,
        ));
    }
    members.sort_by(|a, b| a.0.cmp(&b.0));
    let mut decisions = Vec::new();
    for (ordinal, (_, member)) in members.into_iter().enumerate() {
        let persona = personas
            .iter()
            .find(|p| p.id == member.persona_id)
            .ok_or(Error::UnknownPersona)?;
        let required = selected.contains(&member.persona_id);
        let status = if required {
            "mentioned"
        } else if persona.classifier.is_some() {
            "pending"
        } else {
            "unconfigured"
        };
        if required {
            db.execute("INSERT INTO message_mentions(message_id,persona_id,membership_id) VALUES (?1,?2,?3)",params![user,persona.id,member.membership_id])?;
        }
        let cutoff: i64=db.query_row("SELECT sequence FROM persona_events WHERE persona_id=?1 AND message_id=?2 AND kind='received'",params![persona.id,user],|r|r.get(0))?;
        let id = new_id();
        db.execute("INSERT INTO participation_decisions(id,message_id,conversation_id,persona_id,membership_id,event_cutoff,revision,status,knowledge_version) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![id,user,room.id,persona.id,member.membership_id,cutoff,revision,status,knowledge(db,&persona.id)?])?;
        db.execute("INSERT INTO group_reply_slots(id,round_id,decision_id,ordinal,kind,status) VALUES (?1,?2,?3,?4,?5,?6)",params![new_id(),user,id,ordinal as i64,if required {"required"} else {"optional"},if status=="unconfigured" {"declined"}else{"pending"}])?;
        decisions.push(work(db, &id, persona)?);
    }
    Ok(decisions)
}

impl Database {
    pub(crate) fn queued_reassessment(
        &self,
        persona: &Persona,
        turn: &Turn,
    ) -> Result<Option<DecisionWork>, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let decision:Option<(String,String,i64,i64)>=tx.query_row("SELECT d.id,s.round_id,d.knowledge_version,d.predecessor_version FROM model_calls c JOIN group_reply_slots s ON s.id=c.slot_id JOIN participation_decisions d ON d.id=s.decision_id WHERE c.id=?1 AND c.status='queued' AND s.status='admitted' AND s.kind!='required'",[&turn.call_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let Some((id, round, known, pred)) = decision else {
            return Ok(None);
        };
        if known == knowledge(&tx, &persona.id)? && pred == predecessors(&tx, &round)? {
            return Ok(None);
        }
        let phase: String = tx.query_row(
            "SELECT phase FROM participation_decisions WHERE id=?1",
            [&id],
            |r| r.get(0),
        )?;
        let work = admission::reset(&tx, &id, &round, persona, &phase)?;
        tx.commit()?;
        Ok(Some(work))
    }
    pub(crate) fn step_round(&self, id: &str, personas: &[Persona]) -> Result<Step, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let action = scheduling::step(&tx, id, personas)?;
        tx.commit()?;
        Ok(action)
    }
    pub(crate) fn stop_round(&self, id: &str) -> Result<(), Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        cancel_round(&tx, id)?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn stop_room(&self, room: &str) -> Result<Vec<String>, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let ids=tx.prepare("SELECT id FROM group_rounds WHERE conversation_id=?1 AND status IN ('active','superseded')")?.query_map([room],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        for id in ids {
            cancel_round(&tx, &id)?;
        }
        let calls=tx.prepare("SELECT id FROM model_calls WHERE conversation_id=?1 AND status IN ('queued','streaming')")?.query_map([room],|r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?;
        for call in &calls {
            lifecycle::cancel_call(&tx, call)?;
        }
        tx.commit()?;
        Ok(calls)
    }
    pub(crate) fn call_active(&self, id: &str) -> bool {
        self.connection.lock().expect("database lock").query_row("SELECT EXISTS(SELECT 1 FROM model_calls WHERE id=?1 AND status IN ('queued','streaming'))",[id],|r|r.get(0)).unwrap_or(false)
    }
    pub(crate) fn decision_active(&self, id: &str, generation: i64) -> bool {
        self.connection.lock().expect("database lock").query_row("SELECT EXISTS(SELECT 1 FROM participation_decisions WHERE id=?1 AND generation=?2 AND status='pending')",params![id,generation],|r|r.get(0)).unwrap_or(false)
    }
}
