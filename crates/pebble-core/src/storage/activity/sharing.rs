//! A private proposal may request contact; only a separately admitted reply publishes it.
use super::{Database, activity_request, insert_activity_item, plans};
use crate::{Error, MessageStatus, ModelRequest, Persona};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

fn contact_allowed(
    db: &rusqlite::Connection,
    root: &std::path::Path,
    persona: &str,
) -> Result<bool, Error> {
    let Ok(policy) = super::policy::read(root, persona) else {
        return Ok(false);
    };
    if !policy.active() {
        return Ok(false);
    }
    let (start, end) = policy.quiet_minutes();
    if let (Some(start), Some(end)) = (start, end) {
        let now = super::local_minutes();
        let start = i64::from(start);
        let end = i64::from(end);
        if if start < end {
            now >= start && now < end
        } else {
            now >= start || now < end
        } {
            return Ok(false);
        }
    }
    let busy:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM model_calls WHERE persona_id=?1 AND status IN ('queued','streaming')) OR EXISTS(SELECT 1 FROM activity_calls WHERE persona_id=?1 AND published_message_id IS NOT NULL AND julianday(finished_at)>julianday('now','-20 minutes'))",[persona],|r|r.get(0))?;
    Ok(!busy && !super::scheduler::user_work_pending(db, persona)?)
}

impl Database {
    pub(crate) fn begin_activity_share(
        &self,
        persona: &Persona,
        source: &str,
    ) -> Result<Option<ModelRequest>, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        if !contact_allowed(&tx, &self.root, &persona.id)? {
            return Ok(None);
        }
        let origin:Option<(String,String)>=tx.query_row("SELECT activity_id,session_id FROM activity_calls WHERE id=?1 AND persona_id=?2 AND purpose='private_activity' AND status='completed'",params![source,persona.id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((activity, session)) = origin else {
            return Ok(None);
        };
        let mut q=tx.prepare("SELECT i.id,i.destination_id,i.summary FROM share_intents i JOIN conversations c ON c.id=i.destination_id WHERE i.activity_id=?1 AND i.status='pending' AND i.requested=1 AND c.kind='direct' AND c.persona_id=?2 ORDER BY CASE i.kind WHEN 'commitment' THEN 0 ELSE 1 END,i.rowid")?;
        let candidates = q
            .query_map(params![activity, persona.id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(q);
        let mut chosen = None;
        for candidate in candidates {
            if plans::ready(&tx, &candidate.0)? {
                chosen = Some(candidate);
                break;
            }
        }
        let Some((intent, room, summary)) = chosen else {
            return Ok(None);
        };
        let (schema, protocol): (String, String) = tx.query_row(
            "SELECT response_schema,protocol FROM model_sessions WHERE id=?1 AND status='active'",
            [&session],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let schema: Value = serde_json::from_str(&schema)?;
        let state = super::super::state::current(&tx, &persona.id)?;
        let id = super::super::turns::new_id();
        let step = super::super::turns::new_id();
        tx.execute("INSERT INTO activity_steps(id,activity_id,wake_reason,status) VALUES(?1,?2,'share','running')",params![step,activity])?;
        tx.execute("INSERT INTO activity_calls(id,step_id,activity_id,persona_id,session_id,status,state_before,input_last_sequence,purpose,share_intent_id) VALUES(?1,?2,?3,?4,?5,'streaming',?6,(SELECT COALESCE(MAX(sequence),-1)+1 FROM model_items WHERE session_id=?5),'activity_share',?7)",params![id,step,activity,persona.id,session,serde_json::to_string(&state)?,intent])?;
        let envelope = json!({"purpose":"activity_share","intent":{"id":intent,"summary":summary},"reply_request":{"conversation_id":room},"current_state":state,"temporal_context":super::time::context(&tx,&persona.id,None)?,"instructions":"Rust admitted one private-chat return message for this actual saved intent. Give a concise in-character reply grounded in accepted evidence already in this history. Do not claim successful research from failed/unknown tasks. Return reply/state only. This is a real visible reply, not another private progress update. No tools or new tasks are available."});
        let input = super::super::turns::input_message(&protocol, "user", &envelope.to_string());
        insert_activity_item(&tx, &session, &id, &input)?;
        // Input participates in native history but creates no visible placeholder.
        tx.execute(
            "UPDATE model_items SET kind='input' WHERE activity_call_id=?1",
            [&id],
        )?;
        let request = activity_request(&tx, &self.root, &id, &schema, state, false)?;
        tx.commit()?;
        Ok(Some(request))
    }

    pub(crate) fn finish_activity_share(
        &self,
        request: &ModelRequest,
        native: Option<&[Value]>,
        error: Option<&str>,
    ) -> Result<(), Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let saved:Option<(String,String,String,String)>=tx.query_row("SELECT persona_id,share_intent_id,step_id,state_before FROM activity_calls WHERE id=?1 AND purpose='activity_share' AND status='streaming'",[&request.call_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let Some((persona, intent, step, before)) = saved else {
            return Ok(());
        };
        let allowed = contact_allowed(&tx, &self.root, &persona)? && plans::ready(&tx, &intent)?;
        let mut published = None;
        if allowed && let Some(output) = native {
            let text = crate::live::output_text(output)?;
            let (reply, state) = crate::structured::candidate(
                request.response_schema.as_ref().expect("share schema"),
                &text,
            )
            .map_err(Error::Generation)?;
            if super::super::state::current(&tx, &persona)?
                != serde_json::from_str::<Value>(&before)?
            {
                return Err(Error::Generation(
                    "State changed before activity reply publication.".into(),
                ));
            }
            let message = super::super::turns::insert_message(
                &tx,
                &request.conversation_id,
                "persona",
                &persona,
                &reply,
                MessageStatus::Complete,
            )?;
            super::super::rooms::publish(&tx, &request.conversation_id, &message, Some(&persona))?;
            let event = super::super::turns::insert_event_for(
                &tx,
                &persona,
                &request.conversation_id,
                &message,
                "sent",
            )?;
            for item in output {
                insert_activity_item(&tx, &request.session_id, &request.call_id, item)?;
                tx.execute("INSERT INTO model_item_events(item_id,event_id) SELECT id,?2 FROM model_items WHERE activity_call_id=?1 ORDER BY sequence DESC LIMIT 1",params![request.call_id,event])?;
            }
            tx.execute(
                "UPDATE persona_states SET current_state=?2 WHERE persona_id=?1",
                params![persona, serde_json::to_string(&state)?],
            )?;
            tx.execute("UPDATE share_intents SET status='fulfilled',fulfillment_message_id=?2,requested=0 WHERE id=?1",params![intent,message])?;
            super::super::turns::update_frontier(&tx, &request.session_id, &persona)?;
            published = Some(message);
        }
        let status = if published.is_some() {
            "completed"
        } else if !allowed {
            "cancelled"
        } else {
            "failed"
        };
        tx.execute("UPDATE activity_calls SET status=?2,error=?3,published_message_id=?4,finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",params![request.call_id,status,error,published])?;
        tx.execute("UPDATE activity_steps SET status=?2,finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",params![step,status])?;
        tx.commit()?;
        Ok(())
    }
}
