use super::{Database, rounds};
use crate::Error;
use rusqlite::{Connection, params};

/// The new receipt and supersession commit together. Rejected input cannot stop work.
pub(super) fn supersede(
    db: &Connection,
    room: &str,
    user: &str,
    selected: &mut Vec<String>,
) -> Result<(), Error> {
    // Carry unfinished requested speakers forward, including admitted volunteers.
    // Work in another conversation continues on the shared persona queue.
    let speakers = db.prepare("SELECT d.persona_id FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE d.conversation_id=?1 AND (s.status='admitted' OR (s.status='pending' AND s.kind='required')) ORDER BY d.revision,s.ordinal")?
        .query_map([room], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    for speaker in speakers {
        if !selected.contains(&speaker) {
            selected.push(speaker);
        }
    }
    let calls = db.prepare("SELECT id FROM model_calls WHERE conversation_id=?1 AND status IN ('queued','streaming')")?
        .query_map([room], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let calls = calls
        .into_iter()
        .filter_map(|call| {
            // Background result delivery owns its saved report and attachment independently.
            match (
                super::delegation::report_input(db, &call),
                super::image_generation::report_input(db, &call),
            ) {
                (Ok(None), Ok(None)) => Some(Ok(call)),
                (Err(error), _) | (_, Err(error)) => Some(Err(error)),
                _ => None,
            }
        })
        .collect::<Result<Vec<_>, Error>>()?;
    for call in &calls {
        db.execute(
            "INSERT INTO steered_calls(call_id,user_id) VALUES (?1,?2)",
            params![call, user],
        )?;
    }
    let rounds = db.prepare("SELECT id FROM group_rounds WHERE conversation_id=?1 AND status IN ('active','superseded')")?
        .query_map([room], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    for round in rounds {
        rounds::cancel_round(db, &round)?;
    }
    for call in calls {
        // Reuse the normal cancellation gate, preserving completed native tool effects.
        let reply: String = db.query_row(
            "SELECT reply_message_id FROM model_calls WHERE id=?1",
            [&call],
            |r| r.get(0),
        )?;
        rounds::cancel_publication(db, &call, &reply)?;
    }
    Ok(())
}

impl Database {
    pub(crate) fn steered_calls(&self, user: &str) -> Result<Vec<String>, Error> {
        Ok(self
            .connection
            .lock()
            .expect("database lock")
            .prepare("SELECT call_id FROM steered_calls WHERE user_id=?1")?
            .query_map([user], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?)
    }
}
