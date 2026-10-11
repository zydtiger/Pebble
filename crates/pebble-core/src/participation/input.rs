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

/// Read-only delivered events: all phase-required messages plus bounded older excerpts.
fn view(
    db: &Connection,
    persona: &Persona,
    room: &str,
    cutoff: i64,
    round: &str,
    trigger: &str,
) -> Result<Value, Error> {
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
    let events = |same_room: bool| -> Result<(Vec<Value>, Vec<String>), Error> {
        // Apply access/cutoff rules first. LIMIT applies only to optional older
        // events, so large rounds cannot silently lose required predecessors.
        let mut query = db.prepare(
            "WITH completed AS (
                SELECT c.reply_message_id AS id FROM group_reply_slots s
                JOIN participation_decisions d ON d.id=s.decision_id
                JOIN model_calls c ON c.id=d.call_id
                WHERE s.round_id=?5 AND s.status='completed'
            ), eligible AS (
                SELECT e.sequence,m.id,m.conversation_id,m.sender_kind,m.sender_id,m.content,
                    (m.id=?5 OR m.id=?6 OR m.id IN (SELECT id FROM completed)) AS required
                FROM persona_events e JOIN messages m ON m.id=e.message_id
                WHERE e.persona_id=?1 AND m.status='complete' AND ((m.conversation_id=?3)=?4)
                    AND (e.sequence<=?2 OR m.id IN (SELECT id FROM completed)
                        OR EXISTS(SELECT 1 FROM model_item_events x JOIN model_items i ON i.id=x.item_id
                            JOIN model_sessions z ON z.id=i.session_id WHERE x.event_id=e.id
                            AND z.persona_id=?1 AND z.status='active'))
            ), selected AS (
                SELECT * FROM eligible WHERE required
                UNION ALL
                SELECT * FROM (SELECT * FROM eligible WHERE NOT required ORDER BY sequence DESC LIMIT 8)
            ) SELECT * FROM selected ORDER BY sequence"
        )?;
        let rows = query.query_map(params![persona.id,cutoff,room,same_room,round,trigger], |r| {
            let id: String = r.get(1)?;
            let text: String = r.get(5)?;
            let required: bool = r.get(6)?;
            Ok((json!({"event_sequence":r.get::<_,i64>(0)?,"message_id":id,"conversation_id":r.get::<_,String>(2)?,"speaker":{"kind":r.get::<_,String>(3)?,"id":r.get::<_,String>(4)?},"text":if required {text} else {clip(&text,500)}}), required.then_some(id)))
        })?.collect::<Result<Vec<_>,_>>()?;
        let mut rows = rows;
        for (event, _) in &mut rows {
            event["image_attachments"] = json!(crate::storage::images::references(
                db,
                event["message_id"].as_str().unwrap()
            )?);
        }
        let required = rows.iter().filter_map(|(_, id)| id.clone()).collect();
        Ok((rows.into_iter().map(|(event, _)| event).collect(), required))
    };
    let (private, mut required) = events(false)?;
    let (group, group_required) = events(true)?;
    required.extend(group_required);
    Ok(json!({
        "persona":{"id":persona.id,"name":persona.display_name},
        "SOUL":clip(&persona.soul,3000),
        "MEMORY_current_state":{"MEMORY":clip(&persona.memory,2000),"current_state":state},
        "recent_persona_events":private,
        "recent_group_transcript":group,
        "_required_message_ids":required,
        "context_limits":"Full eligible trigger/root and same-round predecessors; bounded older UTF-8 excerpts in chronological order. No retrieval or memory consolidation is performed."
    }))
}

pub(crate) fn round_input(
    db: &Connection,
    persona: &Persona,
    room: &str,
    cutoff: i64,
    round: &str,
    phase: &str,
    trigger: &str,
) -> Result<String, Error> {
    if phase == "intent" {
        let text: String =
            db.query_row("SELECT content FROM messages WHERE id=?1", [round], |r| {
                r.get(0)
            })?;
        return Ok(serde_json::to_string(
            &json!({"_asset_root":crate::storage::images::root(db)?,"latest_user_message":{"message_id":round,"speaker":"You","content":text,"image_attachments":crate::storage::images::references(db,round)?}}),
        )?);
    }
    let mut input = view(db, persona, room, cutoff, round, trigger)?;
    input["_asset_root"] = json!(crate::storage::images::root(db)?);
    let trigger_event = ["recent_persona_events", "recent_group_transcript"].into_iter()
        .filter_map(|key| input[key].as_array()).flatten()
        .find(|event| event["message_id"] == trigger)
        .ok_or_else(|| Error::Participation("The required participation trigger is not available in this character's eligible delivered events.".into()))?;
    input["latest_trigger"] = json!({"message_id":trigger,"text":trigger_event["text"]});
    if phase != "permission" {
        return Ok(serde_json::to_string(&input)?);
    }
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
        &json!({"_asset_root":input["_asset_root"],"persona":input["persona"],"recent_real_user_transcript":transcript,"silence_constraints":[],"latest_trigger":input["latest_trigger"],"context_limits":"Bounded delivered transcript excerpts, chronological; no inferred permission or interest rules."}),
    )?)
}
