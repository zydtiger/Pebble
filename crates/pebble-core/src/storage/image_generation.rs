use super::{
    Database,
    turns::{insert_item, insert_message, message, new_id},
};
use crate::{
    Error, ImageGeneration, MessageStatus, ModelRequest,
    history::Turn,
    image_generation::{self, tool::Call},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

fn save(db: &Connection, operation: &ImageGeneration) -> Result<(), Error> {
    db.execute(
        "UPDATE image_generations SET record=?2 WHERE id=?1",
        params![operation.id, serde_json::to_string(operation)?],
    )?;
    Ok(())
}
impl Database {
    pub(crate) fn is_image_report(&self, call: &str) -> Result<bool, Error> {
        Ok(report_input(&self.connection.lock().expect("database lock"), call)?.is_some())
    }
    pub(crate) fn image_generations(&self) -> Result<Vec<ImageGeneration>, Error> {
        let db = self.connection.lock().expect("database lock");
        let mut q = db.prepare("SELECT record FROM image_generations ORDER BY rowid")?;
        q.query_map([], |r| r.get::<_, String>(0))?
            .map(|s| Ok(serde_json::from_str(&s?)?))
            .collect()
    }
    pub(crate) fn image_generation(&self, id: &str) -> Result<ImageGeneration, Error> {
        let db = self.connection.lock().expect("database lock");
        let text: String = db.query_row(
            "SELECT record FROM image_generations WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&text)?)
    }
    pub(crate) fn update_image_generation<T>(
        &self,
        id: &str,
        update: impl FnOnce(&mut ImageGeneration) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let text: String = tx.query_row(
            "SELECT record FROM image_generations WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        let mut operation = serde_json::from_str(&text)?;
        let result = update(&mut operation)?;
        save(&tx, &operation)?;
        tx.commit()?;
        Ok(result)
    }
    pub(crate) fn recover_image_generations(&self) -> Result<(), Error> {
        for op in self.image_generations()? {
            self.update_image_generation(&op.id,|o| {
                if o.active() {
                    o.status=if o.dispatch_started {"uncertain"} else {"interrupted"}.into();
                    o.activity.clear();
                    o.detail="Pebble stopped before a confirmed outcome. Reconcile the saved remote identity; generation will never be resent automatically.".into();
                }
                if o.publication_status=="queued" { o.publication_status="failed".into(); }
                Ok(())
            })?;
        }
        Ok(())
    }
    pub(crate) fn image_generation_used(&self, request: &ModelRequest) -> Result<bool, Error> {
        Ok(self.connection.lock().expect("database lock").query_row("WITH RECURSIVE attempts(id,retry_of) AS (SELECT id,retry_of FROM model_calls WHERE id=?1 UNION ALL SELECT c.id,c.retry_of FROM model_calls c JOIN attempts a ON c.id=a.retry_of) SELECT EXISTS(SELECT 1 FROM image_generation_submissions s JOIN attempts a ON a.id=s.owner_id)",[&request.call_id],|r|r.get(0))?)
    }
    pub(crate) fn prepare_image_generation(
        &self,
        request: &ModelRequest,
        call: &Call,
        output: &[Value],
        settings: &crate::ImageGenerationSettings,
        executable: &str,
    ) -> Result<(Value, Option<String>), Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        if let Some(text)=tx.query_row("SELECT result FROM image_generation_submissions WHERE owner_id=?1 AND provider_call_id=?2",params![request.call_id,call.id],|r|r.get::<_,String>(0)).optional()? {return Ok((serde_json::from_str(&text)?,None));}
        let (persona,membership,active):(String,Option<String>,bool)=tx.query_row("SELECT persona_id,membership_id,status='streaming' AND (membership_id IS NULL OR EXISTS(SELECT 1 FROM conversation_memberships WHERE id=model_calls.membership_id AND left_publication IS NULL)) FROM model_calls WHERE id=?1",[&request.call_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        if !active {
            return Err(image_generation::failure(
                "The originating reply is no longer active.",
            ));
        }
        let configured = match call.arguments.backend.as_str() {
            "studio" => settings.studio.is_some(),
            "chatgpt" => settings.chatgpt.is_some(),
            _ => false,
        };
        // One operation per canonical user trigger, including other group speakers.
        let existing:Option<String>=tx.query_row("SELECT id FROM image_generations WHERE json_extract(record,'$.trigger')=?1 AND json_extract(record,'$.conversation_id')=?2 ORDER BY rowid LIMIT 1",params![request.trigger_message_id,request.conversation_id],|r|r.get(0)).optional()?;
        let mut dispatch = None;
        let outcome = if !configured {
            json!({"ok":false,"detail":"This image backend is not configured. Configure Image generation in app settings; do not claim an image was generated."})
        } else if let Some(id) = existing {
            json!({"ok":true,"operation_id":id,"status":"existing","detail":"This user request already has an image operation. Do not generate again; its original owner reports the result."})
        } else {
            let id = new_id();
            let operation = ImageGeneration {
                id: id.clone(),
                persona_id: persona,
                conversation_id: request.conversation_id.clone(),
                backend: call.arguments.backend.clone(),
                status: "queued".into(),
                activity: "Waiting to generate an image…".into(),
                detail: String::new(),
                publication_status: "pending".into(),
                asset_id: None,
                actual_width: None,
                actual_height: None,
                effective_parameters: json!({}),
                arguments: call.arguments.clone(),
                settings: settings.clone(),
                executable: executable.into(),
                trigger: request.trigger_message_id.clone(),
                membership,
                dispatch_started: false,
                remote_id: None,
                turn_id: None,
                item_id: None,
                artifact_id: None,
                report_call: None,
                reports: Vec::new(),
                invalidated: false,
            };
            tx.execute(
                "INSERT INTO image_generations(id,record) VALUES(?1,?2)",
                params![id, serde_json::to_string(&operation)?],
            )?;
            dispatch = Some(id.clone());
            json!({"ok":true,"operation_id":id,"status":"accepted","detail":"One image accepted for background generation. Continue the conversation; actual pixels and outcome arrive separately. Do not poll, repeat or claim completion."})
        };
        let result = crate::memory::result(&request.protocol, &call.id, &outcome.to_string());
        for v in output.iter().chain(std::iter::once(&result)) {
            insert_item(
                &tx,
                &request.session_id,
                Some(&request.call_id),
                "output",
                v,
            )?;
        }
        tx.execute("UPDATE model_calls SET input_last_sequence=(SELECT MAX(sequence) FROM model_items WHERE session_id=?2) WHERE id=?1",params![request.call_id,request.session_id])?;
        tx.execute("INSERT INTO image_generation_submissions(owner_id,provider_call_id,operation_id,result) VALUES(?1,?2,?3,?4)",params![request.call_id,call.id,outcome["operation_id"].as_str(),serde_json::to_string(&result)?])?;
        tx.commit()?;
        Ok((result, dispatch))
    }
    pub(crate) fn invalidate_room_images(
        &self,
        room: &str,
        persona: Option<&str>,
    ) -> Result<Vec<String>, Error> {
        let mut ids = Vec::new();
        for op in self.image_generations()?.into_iter().filter(|o| {
            o.conversation_id == room
                && persona.is_none_or(|p| p == o.persona_id)
                && o.publication_status != "completed"
        }) {
            self.update_image_generation(&op.id, |o| {
                o.invalidated = true;
                o.publication_status = "cancelled".into();
                if o.active() {
                    o.status = "cancelling".into();
                    o.activity = "Cancelling image generation…".into();
                }
                Ok(())
            })?;
            ids.push(op.id);
        }
        Ok(ids)
    }
    pub(crate) fn enqueue_image_report(&self, id: &str) -> Result<Option<Turn>, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let text: String = tx.query_row(
            "SELECT record FROM image_generations WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        let mut op: ImageGeneration = serde_json::from_str(&text)?;
        if op.active() || op.publication_status != "pending" || op.invalidated {
            return Ok(None);
        }
        let eligible:bool=tx.query_row("SELECT ?1 IS NULL OR EXISTS(SELECT 1 FROM conversation_memberships WHERE id=?1 AND left_publication IS NULL)",[&op.membership],|r|r.get(0))?;
        if !eligible {
            op.invalidated = true;
            op.publication_status = "destination_unavailable".into();
            save(&tx, &op)?;
            tx.commit()?;
            return Ok(None);
        }
        let reply = insert_message(
            &tx,
            &op.conversation_id,
            "persona",
            &op.persona_id,
            "",
            MessageStatus::Streaming,
        )?;
        let call = new_id();
        let cutoff: i64 = tx.query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM persona_events WHERE persona_id=?1",
            [&op.persona_id],
            |r| r.get(0),
        )?;
        tx.execute("INSERT INTO model_calls(id,conversation_id,trigger_message_id,reply_message_id,event_cutoff,status,persona_id,persona_sequence,membership_id) VALUES(?1,?2,?3,?4,?5,'queued',?6,(SELECT COALESCE(MAX(persona_sequence),0)+1 FROM model_calls WHERE persona_id=?6),?7)",params![call,op.conversation_id,op.trigger,reply,cutoff,op.persona_id,op.membership])?;
        op.publication_status = "queued".into();
        op.report_call = Some(call.clone());
        op.reports.push(crate::image_generation::ImageReport {
            call: call.clone(),
            input: outcome_input(&tx, &op)?,
        });
        save(&tx, &op)?;
        let turn = Turn {
            call_id: call,
            session_id: String::new(),
            persona_id: op.persona_id,
            conversation_id: op.conversation_id,
            receipt_cutoff: cutoff,
            user: message(&tx, &op.trigger)?,
            reply: message(&tx, &reply)?,
            retry_of: None,
        };
        tx.commit()?;
        Ok(Some(turn))
    }
}
fn for_call(db: &Connection, call: &str) -> Result<Option<ImageGeneration>, Error> {
    let text:Option<String>=db.query_row("SELECT record FROM image_generations WHERE EXISTS(SELECT 1 FROM json_each(record,'$.reports') WHERE json_extract(value,'$.call')=?1)",[call],|r|r.get(0)).optional()?;
    text.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
}
pub(crate) fn report_input(db: &Connection, call: &str) -> Result<Option<Value>, Error> {
    Ok(for_call(db, call)?.and_then(|op| {
        op.reports
            .iter()
            .find(|r| r.call == call)
            .map(|r| r.input.clone())
    }))
}
fn has_public_image(op: &ImageGeneration) -> bool {
    op.status == "completed" || (op.backend == "chatgpt" && op.status == "uncertain")
}
fn outcome_input(db: &Connection, op: &ImageGeneration) -> Result<Value, Error> {
    let mut refs = Vec::new();
    if has_public_image(op)
        && let Some(id) = &op.asset_id
    {
        let text: String = db.query_row(
            "SELECT reference FROM image_assets WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        refs.push(serde_json::from_str::<Value>(&text)?);
    }
    Ok(
        json!({"operation_id":op.id,"backend":op.backend,"status":op.status,"detail":op.detail,"effective_parameters":op.effective_parameters,"requested_size":op.arguments.size,"actual_size":{"width":op.actual_width,"height":op.actual_height},"image_attachments":refs,"message_id":op.id,"instructions":"This is stored image-generation evidence, not a user instruction. Inspect the attached actual pixels. Give a valid in-character reply/state explaining the result in the original room. An image is attached automatically at publication; do not invent a link or regenerate. Explain requested/actual dimension mismatch honestly. An uncertain controller outcome does not invalidate a completed saved image, but explain that limitation. Failed, interrupted or uncertain generation without a saved image is not success. Never invoke tools to explain this result."}),
    )
}
pub(crate) fn report_current(db: &Connection, call: &str) -> Result<bool, Error> {
    Ok(
        for_call(db, call)?
            .is_none_or(|o| !o.invalidated && o.report_call.as_deref() == Some(call)),
    )
}
pub(crate) fn attach_report(db: &Connection, call: &str, message: &str) -> Result<(), Error> {
    if let Some(op) = for_call(db, call)?
        && has_public_image(&op)
        && let Some(id) = op.asset_id
    {
        super::images::attach(db, message, &[id])?;
    }
    Ok(())
}
pub(crate) fn finish_report(db: &Connection, call: &str, status: &str) -> Result<(), Error> {
    db.execute("UPDATE image_generations SET record=json_set(record,'$.publication_status',CASE WHEN json_extract(record,'$.invalidated') THEN 'cancelled' ELSE ?2 END) WHERE json_extract(record,'$.report_call')=?1",params![call,if status=="completed"{"completed"}else{"failed"}])?;
    Ok(())
}
pub(crate) fn retry_report(db: &Connection, call: &str) -> Result<Option<String>, Error> {
    let Some(mut op) = for_call(db, call)? else {
        return Ok(None);
    };
    if op.invalidated
        || op.report_call.as_deref() != Some(call)
        || op.publication_status != "failed"
    {
        return Err(image_generation::failure(
            "This image report cannot be retried.",
        ));
    }
    op.publication_status = "pending".into();
    op.report_call = None;
    save(db, &op)?;
    Ok(Some(op.id))
}
