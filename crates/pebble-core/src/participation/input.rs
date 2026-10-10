use crate::{Error, Persona};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

pub(crate) fn clip(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [excerpt truncated]", &text[..end])
}

/// Read-only bounded views of events already delivered to this persona.
fn view(
    db: &Connection,
    persona: &Persona,
    room: &str,
    cutoff: i64,
    round: &str,
) -> Result<String, Error> {
    let state: String = db.query_row(
        "SELECT current_state FROM persona_states WHERE persona_id=?1",
        [&persona.id],
        |r| r.get(0),
    )?;
    let state: Value = if state.len() <= 4096 {
        serde_json::from_str(&state)?
    } else {
        json!({"truncated_state_excerpt":clip(&state,4096)})
    };
    let events = |same_room: bool| -> Result<Vec<Value>, Error> {
        let mut query = db.prepare("SELECT e.sequence,m.id,m.conversation_id,m.sender_kind,m.sender_id,m.content FROM persona_events e JOIN messages m ON m.id=e.message_id WHERE e.persona_id=?1 AND (e.sequence<=?2 OR EXISTS(SELECT 1 FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id JOIN model_calls c ON c.id=d.call_id WHERE s.round_id=?5 AND s.status='completed' AND c.reply_message_id=m.id) OR EXISTS(SELECT 1 FROM model_item_events x JOIN model_items i ON i.id=x.item_id JOIN model_sessions z ON z.id=i.session_id WHERE x.event_id=e.id AND z.persona_id=?1 AND z.status='active')) AND m.status='complete' AND ((m.conversation_id=?3)=?4) ORDER BY e.sequence DESC LIMIT 8")?;
        let mut rows = query.query_map(params![persona.id,cutoff,room,same_room,round], |r|Ok(json!({"event_sequence":r.get::<_,i64>(0)?,"message_id":r.get::<_,String>(1)?,"conversation_id":r.get::<_,String>(2)?,"speaker":{"kind":r.get::<_,String>(3)?,"id":r.get::<_,String>(4)?},"text":clip(&r.get::<_,String>(5)?,500)})))?.collect::<Result<Vec<_>,_>>()?;
        rows.reverse();
        Ok(rows)
    };
    Ok(serde_json::to_string(&json!({
        "persona":{"id":persona.id,"name":persona.display_name},
        "SOUL":clip(&persona.soul,3000),
        "MEMORY_current_state":{"MEMORY":clip(&persona.memory,2000),"current_state":state},
        "recent_persona_events":events(false)?,
        "recent_group_transcript":events(true)?,
        "context_limits":"UTF-8 excerpts only; latest group message is last. No retrieval or memory consolidation is performed."
    }))?)
}

pub(crate) fn round_input(
    db: &Connection,
    persona: &Persona,
    room: &str,
    cutoff: i64,
    round: &str,
    phase: &str,
) -> Result<String, Error> {
    if phase == "intent" {
        let text: String =
            db.query_row("SELECT content FROM messages WHERE id=?1", [round], |r| {
                r.get(0)
            })?;
        return Ok(serde_json::to_string(
            &json!({"latest_user_message":{"speaker":"You","content":clip(&text,8000)}}),
        )?);
    }
    let input = view(db, persona, room, cutoff, round)?;
    if phase != "permission" {
        return Ok(input);
    }
    let input: Value = serde_json::from_str(&input)?;
    let mut transcript = input["recent_persona_events"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    transcript.extend(
        input["recent_group_transcript"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    transcript.retain(|event| event["speaker"]["kind"] == "user");
    transcript.sort_by_key(|event| event["event_sequence"].as_i64().unwrap_or(0));
    Ok(serde_json::to_string(
        &json!({"persona":input["persona"],"recent_real_user_transcript":transcript,"silence_constraints":[],"context_limits":"Bounded delivered transcript excerpts, chronological; no inferred permission or interest rules."}),
    )?)
}
