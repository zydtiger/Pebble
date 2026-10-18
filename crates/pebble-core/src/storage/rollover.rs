use super::{Database, maintenance::Job};
use crate::{Error, Persona, history::Turn};
use rusqlite::{OptionalExtension, params};

impl Database {
    pub(crate) fn record_rollover_failure(
        &self,
        persona: &Persona,
        error: &Error,
    ) -> Result<(), Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let source:Option<(String,i64)>=tx.query_row("SELECT s.id,COALESCE(MAX(i.sequence),0) FROM model_sessions s JOIN model_items i ON i.session_id=s.id WHERE s.persona_id=?1 AND s.status='active' GROUP BY s.id",[&persona.id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((source, last)) = source {
            tx.execute("INSERT OR IGNORE INTO automatic_rollovers(source_session,last_item_sequence) VALUES(?1,?2)",params![source,last])?;
            tx.execute("INSERT INTO context_maintenance(id,persona_id,source_session,event_cutoff,state_before,binding,status,detail,automatic) VALUES(?1,?2,?3,0,?4,?5,'failed',?6,1)",params![super::turns::new_id(),persona.id,source,super::state::current(&tx,&persona.id)?.to_string(),serde_json::to_string(&crate::connection::identity(persona.chat.as_ref()))?,format!("Automatic memory review could not start: {error} The old context remains active. Inspect the reported problem, then use Start fresh context to retry.")])?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Called only by the current queue owner, before releasing its reservation.
    pub(crate) fn automatic_rollover(
        &self,
        persona: &Persona,
        pending: Option<&Turn>,
    ) -> Result<Option<Job>, Error> {
        let Some(chat) = &persona.chat else {
            return Ok(None);
        };
        let source = {
            let db = self.connection.lock().expect("database lock");
            if db.query_row("SELECT EXISTS(SELECT 1 FROM context_maintenance WHERE persona_id=?1 AND status='running')",[&persona.id],|r|r.get::<_,bool>(0))? { return Ok(None) }
            let session: Option<String> = db.query_row(
                "SELECT id FROM model_sessions WHERE persona_id=?1 AND status='active' AND memory_tools=1 AND response_schema IS NOT NULL AND protocol=?2 AND model=?3 AND base_url=?4",
                params![persona.id,chat.api,chat.model,crate::connection::identity(persona.chat.as_ref()).2], |r| r.get(0),
            ).optional()?;
            let Some(session) = session else {
                return Ok(None);
            };
            let call: Option<String> = db.query_row("SELECT id FROM model_calls WHERE session_id=?1 AND status='completed' ORDER BY persona_sequence DESC LIMIT 1",[&session],|r|r.get(0)).optional()?;
            let private: Option<(String, String)> = if call.is_none() {
                db.query_row("SELECT c.id,s.response_schema FROM activity_calls c JOIN model_sessions s ON s.id=c.session_id WHERE c.session_id=?1 AND c.status='completed' ORDER BY c.rowid DESC LIMIT 1",[&session],|r|Ok((r.get(0)?,r.get(1)?))).optional()?
            } else {
                None
            };
            if call.is_none() && private.is_none() {
                return Ok(None);
            }
            let last: i64 = db.query_row(
                "SELECT MAX(sequence) FROM model_items WHERE session_id=?1",
                [&session],
                |r| r.get(0),
            )?;
            if db.query_row("SELECT EXISTS(SELECT 1 FROM automatic_rollovers WHERE source_session=?1 AND last_item_sequence=?2)",params![session,last],|r|r.get::<_,bool>(0))? { return Ok(None) }
            let busy: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM model_calls WHERE persona_id=?1 AND status='queued') OR EXISTS(SELECT 1 FROM participation_decisions WHERE persona_id=?1 AND status='pending') OR EXISTS(SELECT 1 FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE d.persona_id=?1 AND s.status IN ('pending','admitted'))",[&persona.id],|r|r.get(0))?;
            let request = if let Some(call) = call {
                super::items::request(&db, &self.root, &call, true)?
            } else {
                let (call, schema) = private.expect("completed activity");
                let schema = crate::structured::private_schema(&serde_json::from_str(&schema)?)
                    .map_err(Error::Generation)?;
                super::activity::activity_request(
                    &db,
                    &self.root,
                    &call,
                    &schema,
                    super::state::current(&db, &persona.id)?,
                    true,
                )?
            };
            (session, last, busy, request)
        };
        let (session, last, busy, mut request) = source;
        // A prefix which itself reaches the threshold cannot be fixed by rollover.
        let full = crate::images::estimate(
            &crate::live::request_body(chat, &request, persona.context.max_output_tokens)?.1,
        )?;
        request.items.retain(|i| i.sequence == 0);
        request.recent_history = None; // Carry can shrink; only the authored prefix is fixed.
        let fixed = crate::images::estimate(
            &crate::live::request_body(chat, &request, persona.context.max_output_tokens)?.1,
        )?;
        if fixed >= persona.context.chat_ceiling() * 8 / 10 {
            return Ok(None);
        }
        let projected = pending
            .map(|turn| self.preview_turn(persona, turn))
            .transpose()?;
        let projected = projected
            .as_ref()
            .map(|r| {
                crate::live::request_body(chat, r, persona.context.max_output_tokens)
                    .and_then(|(_, body)| crate::images::estimate(&body))
            })
            .transpose()?;
        if !persona.context.rollover_due(full, busy)
            && !projected.is_some_and(|n| n >= persona.context.chat_ceiling() * 9 / 10)
        {
            return Ok(None);
        }
        self.connection.lock().expect("database lock").execute(
            "INSERT INTO automatic_rollovers(source_session,last_item_sequence) VALUES(?1,?2)",
            params![session, last],
        )?;
        self.begin_context_maintenance(persona, true).map(Some)
    }
}
