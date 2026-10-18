//! Validate model-authored activity effects without accepting partial changes.
use super::*;

pub(super) fn verify_outcomes(frozen: &str, decision: &Value) -> Result<(), Error> {
    let mut expected: Vec<Value> = serde_json::from_str(frozen)?;
    let mut observed = decision["observed_outcomes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let sort = |values: &mut Vec<Value>| values.sort_by_key(Value::to_string);
    sort(&mut expected);
    sort(&mut observed);
    if expected != observed {
        return Err(Error::Generation("observed_outcomes must acknowledge exactly the frozen task_outcomes task_id/outcome_revision pairs, once each.".into()));
    }
    if !expected.is_empty()
        && decision["summary"]
            .as_str()
            .is_none_or(|s| s.trim().is_empty())
    {
        return Err(Error::Generation(
            "Summarize the observed task outcome, including failures or limitations.".into(),
        ));
    }
    Ok(())
}

/// Only model plan validation errors are isolated. Database/configuration failures
/// still abort publication. The entire proposal rolls back, including earlier ops.
pub(crate) fn apply_chat(
    db: &rusqlite::Connection,
    root: &std::path::Path,
    persona: &str,
    room: &str,
    message: Option<&str>,
    proposal: &Value,
) -> Result<Option<String>, Error> {
    db.execute_batch("SAVEPOINT chat_activity_plan")?;
    let result = plans::apply(db, root, persona, room, message, proposal);
    if result.is_err() {
        db.execute_batch("ROLLBACK TO chat_activity_plan")?;
    }
    db.execute_batch("RELEASE chat_activity_plan")?;
    match result {
        Ok(()) => Ok(None),
        Err(Error::Generation(detail)) => Ok(Some(detail)),
        Err(error) => Err(error),
    }
}

impl Database {
    pub(crate) fn chat_activity_feedback(
        &self,
        request: &crate::ModelRequest,
        native: &[Value],
    ) -> Result<Option<String>, Error> {
        if request
            .response_schema
            .as_ref()
            .is_none_or(|s| s["properties"].get("activity").is_none())
        {
            return Ok(None);
        }
        let text = crate::live::output_text(native)?;
        let value: Value = serde_json::from_str(&text)?;
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let (persona, message): (String, String) = tx.query_row(
            "SELECT persona_id,reply_message_id FROM model_calls WHERE id=?1",
            [&request.call_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let feedback = apply_chat(
            &tx,
            &self.root,
            &persona,
            &request.conversation_id,
            Some(&message),
            &value["activity"],
        )?;
        tx.rollback()?;
        Ok(feedback)
    }

    pub(crate) fn private_activity_feedback(
        &self,
        request: &crate::ModelRequest,
        native: &[Value],
    ) -> Result<Option<String>, Error> {
        let text = crate::live::output_text(native)?;
        let schema = request
            .response_schema
            .as_ref()
            .ok_or_else(|| Error::Generation("Private schema is missing.".into()))?;
        let (_, decision) = match crate::structured::private_candidate(schema, &text) {
            Ok(value) => value,
            Err(detail) => return Ok(Some(detail)),
        };
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let (persona, activity, outcomes): (String,String,String) = tx.query_row("SELECT persona_id,activity_id,outcome_ids FROM activity_calls WHERE id=?1 AND status='streaming'",[&request.call_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let check = || -> Result<(), Error> {
            verify_outcomes(&outcomes, &decision)?;
            plans::apply(
                &tx,
                &self.root,
                &persona,
                &request.conversation_id,
                None,
                &decision["plans"],
            )?;
            if let Some(id) = decision["share_intent_id"].as_str() {
                let owned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM share_intents WHERE id=?1 AND activity_id=?2 AND status='pending' AND destination_id=?3)",params![id,activity,request.conversation_id],|r|r.get(0))?;
                if !owned {
                    return Err(Error::Generation(
                        "Choose an actual pending intent for the private chat.".into(),
                    ));
                }
            }
            Ok(())
        };
        let result = check();
        tx.rollback()?;
        match result {
            Ok(()) => Ok(None),
            Err(Error::Generation(detail)) => Ok(Some(detail)),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn record_activity_validation(
        &self,
        request: &crate::ModelRequest,
        detail: &str,
    ) -> Result<(), Error> {
        let db = self.connection.lock().expect("database lock");
        db.execute("INSERT INTO activity_events(id,activity_id,ordinal,kind,summary,provenance) SELECT ?1,c.activity_id,(SELECT COALESCE(MAX(ordinal),0)+1 FROM activity_events WHERE activity_id=c.activity_id),'validation_rejected',?3,'runtime' FROM activity_calls c WHERE c.id=?2",params![Uuid::new_v4().to_string(),request.call_id,crate::participation::clip(detail,480)])?;
        Ok(())
    }
}
