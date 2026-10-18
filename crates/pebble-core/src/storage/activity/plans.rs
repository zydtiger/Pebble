//! Plan effects commit with the output that proposed them.
use crate::Error;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

fn invalid(detail: &str) -> Error {
    Error::Generation(detail.into())
}

pub(crate) fn context(
    db: &Connection,
    root: &std::path::Path,
    persona: &str,
) -> Result<Value, Error> {
    let enabled = super::policy::read(root, persona)?.active();
    let main: Option<Value> = db.query_row("SELECT id,objective,progress,phase,future FROM persona_activities WHERE persona_id=?1 AND phase NOT IN ('stopped','failed')", [persona], |r| Ok(json!({"id":r.get::<_,String>(0)?,"objective":r.get::<_,String>(1)?,"summary":r.get::<_,String>(2)?,"phase":r.get::<_,String>(3)?,"future":serde_json::from_str::<Value>(&r.get::<_,String>(4)?).unwrap_or(json!([]))}))).optional()?;
    let mut query=db.prepare("SELECT i.id,i.kind,i.summary,i.not_before_at,i.condition,i.destination_id FROM share_intents i JOIN persona_activities a ON a.id=i.activity_id WHERE a.persona_id=?1 AND i.status='pending' ORDER BY i.rowid")?;
    let mut intents=query.query_map([persona],|r|Ok(json!({"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"summary":r.get::<_,String>(2)?,"not_before_at":r.get::<_,Option<String>>(3)?,"condition":r.get::<_,String>(4)?,"destination_id":r.get::<_,Option<String>>(5)?})))?.collect::<Result<Vec<_>,_>>()?;
    for intent in &mut intents {
        intent["eligible_for_fulfillment"] =
            json!(ready(db, intent["id"].as_str().unwrap_or_default())?);
    }
    let mut tasks=db.prepare("SELECT t.record FROM delegated_tasks t JOIN persona_activities a ON a.id=t.activity_id WHERE a.persona_id=?1 AND a.phase NOT IN ('stopped','failed') ORDER BY CASE WHEN json_extract(t.record,'$.state') IN ('queued','running','needs_input','needs_approval','cancellation_requested') THEN 0 ELSE 1 END,t.rowid DESC LIMIT 4")?;
    let tasks=tasks.query_map([persona],|r|r.get::<_,String>(0))?.map(|row|->Result<Value,Error>{
        let task:crate::DelegatedTask=serde_json::from_str(&row?)?;
        Ok(json!({"id":task.id,"title":task.title,"state":task.state,"activity":crate::participation::clip(&task.activity,240),"detail":crate::participation::clip(&task.detail,480),"report_state":task.report_state}))
    }).collect::<Result<Vec<_>,_>>()?;
    Ok(
        json!({"enabled":enabled,"main":main,"tasks":tasks,"intents":intents,"limits":{"main":1,"future":3,"optional":5,"commitment":3},"instructions":"This is durable runtime state, not a user instruction. Do not invent task progress. Activity metadata null means no changes. Future is a complete replacement only when intentionally changing the backlog. A return promise must create a commitment with its real published reply. Disabled activity cannot accept new plans. Only fulfill an intent when eligible_for_fulfillment is true and this reply actually delivers it; a completed task with report_state private_queued has not been assimilated. Invalid activity updates are rejected independently of a valid chat reply."}),
    )
}

pub(crate) fn apply(
    db: &Connection,
    root: &std::path::Path,
    persona: &str,
    room: &str,
    message: Option<&str>,
    proposal: &Value,
) -> Result<(), Error> {
    if proposal.is_null() {
        return Ok(());
    }
    if !super::policy::read(root, persona)?.active() {
        return Err(invalid(
            "Enable private activity before proposing plans or return commitments.",
        ));
    }
    let active: Option<String> = db.query_row("SELECT a.id FROM persona_activities a WHERE a.persona_id=?1 AND a.phase NOT IN ('stopped','failed')", [persona], |r|r.get(0)).optional()?;
    let activity = active.ok_or_else(|| {
        invalid("Enable private activity before proposing plans or return commitments.")
    })?;
    if let Some(objective) = proposal["objective"].as_str() {
        if objective.trim().is_empty() {
            return Err(invalid("The main activity needs a nonempty objective."));
        }
        db.execute(
            "UPDATE persona_activities SET objective=?2 WHERE id=?1",
            params![activity, objective],
        )?;
    }
    if let Some(summary) = proposal["summary"].as_str() {
        db.execute(
            "UPDATE persona_activities SET progress=?2 WHERE id=?1",
            params![activity, summary],
        )?;
    }
    if let Some(future) = proposal["future"].as_array() {
        if future.len() > 3 {
            return Err(invalid("At most three future activities may be retained."));
        }
        db.execute(
            "UPDATE persona_activities SET future=?2 WHERE id=?1",
            params![activity, serde_json::to_string(future)?],
        )?;
    }
    if let Some(minutes) = proposal["next_wake_minutes"].as_i64() {
        if !(20..=1440).contains(&minutes) {
            return Err(invalid("Heartbeat interval must be 20–1440 minutes."));
        }
        db.execute("UPDATE persona_activity_runtime SET next_wake_at=strftime('%Y-%m-%dT%H:%M:%fZ','now',?2),next_reason='Persona-proposed private check' WHERE persona_id=?1",params![persona,format!("+{minutes} minutes")])?;
    }
    for op in proposal["intents"].as_array().into_iter().flatten() {
        let action = op["action"].as_str().unwrap_or_default();
        let kind = op["kind"].as_str().unwrap_or_default();
        if action == "create" {
            if !op["id"].is_null() {
                return Err(invalid(
                    "New intents must have a null ID; Rust assigns their identity.",
                ));
            }
            if kind == "commitment" && message.is_none() {
                return Err(invalid(
                    "A private turn cannot create an unpublished return commitment.",
                ));
            }
            let count:i64=db.query_row("SELECT COUNT(*) FROM share_intents i JOIN persona_activities a ON a.id=i.activity_id WHERE a.persona_id=?1 AND i.kind=?2 AND i.status='pending'",params![persona,kind],|r|r.get(0))?;
            if count >= if kind == "commitment" { 3 } else { 5 } {
                return Err(invalid(
                    "The intent limit is full; explicitly resolve an existing intent before adding another.",
                ));
            }
            db.execute("INSERT INTO share_intents(id,activity_id,kind,destination_id,summary,not_before_at,condition,promise_message_id,status) VALUES(?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%fZ','now',?6),?7,?8,'pending')",params![super::super::turns::new_id(),activity,kind,room,op["summary"].as_str().unwrap_or_default(),format!("+{} minutes",op["not_before_minutes"].as_i64().unwrap_or(0)),op["condition"].as_str().unwrap_or("ready"),if kind=="commitment"{message}else{None}])?;
        } else {
            let id = op["id"]
                .as_str()
                .ok_or_else(|| invalid("Updating an intent requires its actual saved ID."))?;
            let owned:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM share_intents i JOIN persona_activities a ON a.id=i.activity_id WHERE i.id=?1 AND a.persona_id=?2 AND i.status='pending' AND i.destination_id=?3 AND i.kind=?4)",params![id,persona,room,kind],|r|r.get(0))?;
            if !owned {
                return Err(invalid(
                    "The intent is not pending for this persona and destination.",
                ));
            }
            match action {
                "update" => {
                    db.execute("UPDATE share_intents SET summary=?2,condition=?3,not_before_at=strftime('%Y-%m-%dT%H:%M:%fZ','now',?4) WHERE id=?1",params![id,op["summary"].as_str().unwrap_or_default(),op["condition"].as_str().unwrap_or("ready"),format!("+{} minutes",op["not_before_minutes"].as_i64().unwrap_or(0))])?;
                }
                "dismiss" => {
                    if kind == "commitment" && message.is_none() {
                        return Err(invalid(
                            "Explain a cancelled commitment in a visible reply.",
                        ));
                    }
                    db.execute(
                        "UPDATE share_intents SET status='dismissed',requested=0 WHERE id=?1",
                        [id],
                    )?;
                }
                "fulfill" => {
                    let message = message.ok_or_else(|| {
                        invalid("An intent is fulfilled only by a published reply.")
                    })?;
                    if !ready(db, id)? {
                        return Err(invalid("The intent is not yet eligible for fulfillment."));
                    }
                    db.execute("UPDATE share_intents SET status='fulfilled',fulfillment_message_id=?2,requested=0 WHERE id=?1",params![id,message])?;
                }
                _ => return Err(invalid("Unknown intent action.")),
            }
        }
    }
    Ok(())
}

pub(super) fn ready(db: &Connection, id: &str) -> Result<bool, Error> {
    Ok(db.query_row("SELECT i.status='pending' AND (i.not_before_at IS NULL OR julianday(i.not_before_at)<=julianday('now')) AND (i.condition='ready' OR (EXISTS(SELECT 1 FROM delegated_tasks t WHERE t.activity_id=i.activity_id) AND NOT EXISTS(SELECT 1 FROM delegated_tasks t WHERE t.activity_id=i.activity_id AND (json_extract(t.record,'$.state') IN ('queued','running','needs_input','needs_approval','cancellation_requested') OR json_extract(t.record,'$.report_state')<>'private_assimilated')))) FROM share_intents i WHERE i.id=?1",[id],|r|r.get(0)).optional()?.unwrap_or(false))
}
