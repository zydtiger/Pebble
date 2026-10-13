use super::{
    Database,
    turns::{input_message, new_id},
};
use crate::{ContextMarker, Error, ModelItem, ModelRequest, Persona};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
#[derive(Clone)]
pub(crate) struct Job {
    pub id: String,
    pub source: Option<String>,
    pub cutoff: i64,
    pub state: Value,
    pub memory: String,
    pub binding: (String, String, String),
    pub request: Option<ModelRequest>,
    pub has_knowledge: bool,
}
struct SavedBoundary {
    id: String,
    source: Option<String>,
    cutoff: i64,
    state: String,
    memory: String,
    request: String,
    binding: String,
}
impl Database {
    pub(crate) fn begin_maintenance(&self, persona: &Persona) -> Result<Job, Error> {
        self.ensure_memory_resolved(&persona.id)?;
        self.ensure_idle(Some(&persona.id))?;
        self.ensure_decisions_idle(Some(&persona.id))?;
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let binding = crate::connection::identity(persona.chat.as_ref());
        let cutoff: i64 = tx.query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM persona_events WHERE persona_id=?1",
            [&persona.id],
            |r| r.get(0),
        )?;
        let source:Option<(String,i64,String,String,String)>=tx.query_row("SELECT id,event_start_sequence,protocol,model,base_url FROM model_sessions WHERE persona_id=?1 AND status='active'",[&persona.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        if let Some((_, _, protocol, model, base)) = &source {
            let (p, m, b) = crate::connection::identity(persona.chat.as_ref());
            if protocol != &p || model != &m || base != &b {
                return Err(Error::Generation("Connection changed. Start one chat with the new connection to establish its separate context before memory maintenance. No outgoing native history was sent to the changed provider.".into()));
            }
        }
        crate::structured::validate_state(
            &persona.response_schema,
            &super::state::current(&tx, &persona.id)?,
        )
        .map_err(|detail| {
            Error::Generation(format!(
                "Current state is incompatible with the response schema: {detail}"
            ))
        })?;
        let state = super::state::current(&tx, &persona.id)?;
        let memory =
            super::memory::read(&std::path::Path::new(&persona.directory).join("MEMORY.md"))?;
        let recovery = tx.query_row("SELECT j.id,j.source_session,j.event_cutoff,j.state_before,o.content,j.request,j.binding FROM context_maintenance j JOIN memory_operations o ON o.owner_id=j.id WHERE j.persona_id=?1 AND j.status IN ('failed','cancelled','interrupted') AND o.status='committed' ORDER BY j.rowid DESC LIMIT 1",[&persona.id],|r|Ok(SavedBoundary { id:r.get(0)?,source:r.get(1)?,cutoff:r.get(2)?,state:r.get(3)?,memory:r.get(4)?,request:r.get(5)?,binding:r.get(6)? })).optional()?;
        let id = new_id();
        let (session, start, protocol, model) = source.clone().map_or_else(
            || {
                let (p, m, _) = crate::connection::identity(persona.chat.as_ref());
                (String::new(), 0, p, m)
            },
            |(id, start, p, m, _)| (id, start, p, m),
        );
        let mut items = Vec::new();
        let mut has_knowledge = false;
        if !session.is_empty() {
            let mut q=tx.prepare("SELECT id,sequence,codec,payload,decoded_length,kind FROM model_items WHERE session_id=?1 ORDER BY sequence")?;
            // Metadata preflight precedes materialization, including opaque reasoning.
            let mut metadata = tx.prepare(
                "SELECT codec,decoded_length,length(payload) FROM model_items WHERE session_id=?1",
            )?;
            let mut remaining = super::codec::DECODED_CAP;
            for row in metadata.query_map([&session], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })? {
                let (codec, length, stored) = row?;
                let n = super::codec::metadata(&codec, length, stored)?;
                remaining = remaining
                    .checked_sub(n + super::codec::ITEM_OVERHEAD)
                    .ok_or_else(super::codec::cap_error)?;
            }
            let mut remaining = super::codec::DECODED_CAP;
            let mut rows = q.query([&session])?;
            while let Some(r) = rows.next()? {
                remaining -= super::codec::ITEM_OVERHEAD;
                let payload = super::codec::decode(
                    &r.get::<_, String>(2)?,
                    r.get(3)?,
                    r.get::<_, i64>(4)? as usize,
                    remaining,
                )?;
                remaining -= payload.len();
                if r.get::<_, String>(5)? != "prefix" {
                    has_knowledge = true;
                }
                items.push(ModelItem {
                    id: r.get(0)?,
                    sequence: r.get(1)?,
                    payload,
                });
            }
        }
        if let Some(SavedBoundary {
            id: old_id,
            source: old_source,
            cutoff: old_cutoff,
            state: old_state,
            memory: saved,
            request: frozen,
            binding: old_binding,
        }) = recovery
        {
            let old_state: Value = serde_json::from_str(&old_state)?;
            let mut request: ModelRequest = serde_json::from_str(&frozen)?;
            request.image_root = self.root.clone();
            let same_native = old_source == source.as_ref().map(|s| s.0.clone())
                && (old_source.is_none()
                    || (request.items.len() == items.len() + 1
                        && request.items.iter().zip(&items).all(|(old, current)| {
                            old.id == current.id
                                && old.sequence == current.sequence
                                && old.payload == current.payload
                        })));
            let old_binding: (String, String, String) = serde_json::from_str(&old_binding)?;
            if same_native && old_state == state && saved == memory && old_binding == binding {
                tx.execute(
                    "UPDATE context_maintenance SET status='running',detail='' WHERE id=?1",
                    [&old_id],
                )?;
                tx.commit()?;
                return Ok(Job {
                    id: old_id,
                    source: old_source,
                    cutoff: old_cutoff,
                    state,
                    memory,
                    binding,
                    request: Some(request),
                    has_knowledge: true,
                });
            }
            // The saved effect remains archived. A new explicit action reviews newer knowledge;
            // it must not publish the stale cutoff or repeat the old prepared effect.
            tx.execute("UPDATE context_maintenance SET status='superseded',detail=detail || ' Saved memory retained; a newer explicit review superseded this unfinished boundary.' WHERE id=?1",[old_id])?;
        }
        let mut events = Vec::new();
        {
            let mut q=tx.prepare("SELECT e.id,e.sequence,e.conversation_id,m.sender_kind,m.sender_id,m.content,COALESCE(p.display_name,'You'),m.id FROM persona_events e JOIN messages m ON m.id=e.message_id LEFT JOIN personas p ON p.id=m.sender_id WHERE e.persona_id=?1 AND e.sequence>?2 AND e.sequence<=?3 AND e.kind='received' AND NOT EXISTS(SELECT 1 FROM model_item_events x JOIN model_items i ON i.id=x.item_id WHERE x.event_id=e.id AND i.session_id=?4) AND NOT EXISTS(SELECT 1 FROM model_calls c WHERE c.trigger_message_id=e.message_id AND c.persona_id=e.persona_id AND c.session_id IS NULL AND c.status IN ('cancelled','interrupted','failed') AND (SELECT kind FROM conversations WHERE id=e.conversation_id)='direct') ORDER BY e.sequence")?;
            for row in q.query_map(params![persona.id,start,cutoff,session],|r|Ok(json!({"message_id":r.get::<_,String>(7)?,"event_id":r.get::<_,String>(0)?,"sequence":r.get::<_,i64>(1)?,"conversation_id":r.get::<_,String>(2)?,"speaker":{"kind":r.get::<_,String>(3)?,"id":r.get::<_,String>(4)?,"name":r.get::<_,String>(6)?},"text":r.get::<_,String>(5)?})))? {let mut event=row?;event["image_attachments"]=json!(super::images::references(&tx,event["message_id"].as_str().unwrap())?);events.push(event);}
        }
        has_knowledge |= !events.is_empty();
        let instructions = json!({"maintenance":"Review the whole outgoing knowledge and current MEMORY below. Preserve useful existing memory, correct outdated facts, distinguish fictional/real context, and exclude current affinity/mood state. You MUST call update_memory once with the complete revised MEMORY, even when unchanged. Do not reply to the user or update accepted state. No conversational response is requested.","current_memory":memory,"current_state":state,"eligible_observations":events,"rules":crate::memory::INSTRUCTIONS});
        if items.is_empty() {
            items.push(ModelItem {
                id: String::new(),
                sequence: 0,
                payload: serde_json::to_vec(&input_message(
                    &protocol,
                    "system",
                    &format!("You are {}. SOUL\n{}", persona.display_name, persona.soul),
                ))?,
            });
        }
        items.push(ModelItem {
            id: String::new(),
            sequence: i64::MAX,
            payload: serde_json::to_vec(&crate::images::input_message(&protocol, &instructions)?)?,
        });
        let request = ModelRequest {
            image_root: self.root.clone(),
            call_id: id.clone(),
            session_id: session.clone(),
            conversation_id: persona.conversation_id.clone(),
            trigger_message_id: String::new(),
            event_cutoff: cutoff,
            protocol,
            model,
            items,
            response_schema: None,
            state_before: Some(state.clone()),
            memory_tools: true,
            memory_capable: true,
            delegation_tools: false,
            image_generation_tools: false,
            image_generation_backends: Vec::new(),
        };
        tx.execute("INSERT INTO context_maintenance(id,persona_id,source_session,event_cutoff,state_before,status,request,binding) VALUES(?1,?2,?3,?4,?5,'running',?6,?7)",params![id,persona.id,source.as_ref().map(|s|&s.0),cutoff,state.to_string(),serde_json::to_string(&request)?,serde_json::to_string(&binding)?])?;
        tx.commit()?;
        Ok(Job {
            id,
            source: source.map(|s| s.0),
            cutoff,
            state,
            memory,
            binding,
            request: Some(request),
            has_knowledge,
        })
    }
    pub(crate) fn archive_maintenance(&self, id: &str, output: &[Value]) -> Result<(), Error> {
        self.connection.lock().expect("database lock").execute(
            "UPDATE context_maintenance SET native_output=?2 WHERE id=?1",
            params![id, serde_json::to_string(output)?],
        )?;
        Ok(())
    }
    pub(crate) fn archive_maintenance_result(&self, id: &str, result: &Value) -> Result<(), Error> {
        self.connection.lock().expect("database lock").execute(
            "UPDATE context_maintenance SET native_result=?2 WHERE id=?1",
            params![id, result.to_string()],
        )?;
        Ok(())
    }
    pub(crate) fn publish_maintenance(&self, job: &Job, persona: &Persona) -> Result<(), Error> {
        if crate::connection::identity(persona.chat.as_ref()) != job.binding {
            return Err(Error::Generation("Connection changed during memory maintenance; the old context was preserved. Start a chat with the new connection to establish its separate context.".into()));
        }
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let source: Option<String> = tx
            .query_row(
                "SELECT id FROM model_sessions WHERE persona_id=?1 AND status='active'",
                [&persona.id],
                |r| r.get(0),
            )
            .optional()?;
        if source != job.source || super::state::current(&tx, &persona.id)? != job.state {
            return Err(Error::Generation("Context or accepted state changed during maintenance; the old context was preserved.".into()));
        }
        let expected: Option<String> = tx
            .query_row(
                "SELECT content FROM memory_operations WHERE owner_id=?1 AND status='committed'",
                [&job.id],
                |r| r.get(0),
            )
            .optional()?;
        if persona.memory != expected.unwrap_or_else(|| job.memory.clone()) {
            return Err(Error::Generation(
                "MEMORY changed before context publication; inspect the file before retrying."
                    .into(),
            ));
        }
        let session = super::contexts::create_session(&tx, persona, job.cutoff, "memory")?;
        let marker = new_id();
        tx.execute(
            "INSERT INTO context_markers(id,persona_id,session_id) VALUES(?1,?2,?3)",
            params![marker, persona.id, session],
        )?;
        tx.execute("INSERT INTO context_marker_rooms(marker_id,conversation_id,after_sequence) SELECT ?1,c.id,COALESCE((SELECT MAX(sequence) FROM messages WHERE conversation_id=c.id),0) FROM conversations c WHERE c.persona_id=?2 OR EXISTS(SELECT 1 FROM conversation_memberships m WHERE m.conversation_id=c.id AND m.persona_id=?2 AND m.left_publication IS NULL)",params![marker,persona.id])?;
        tx.execute(
            "UPDATE context_maintenance SET status='completed' WHERE id=?1",
            [&job.id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn finish_maintenance(
        &self,
        id: &str,
        status: &str,
        detail: &str,
    ) -> Result<(), Error> {
        self.connection.lock().expect("database lock").execute(
            "UPDATE context_maintenance SET status=?2,detail=?3 WHERE id=?1",
            params![id, status, detail],
        )?;
        Ok(())
    }
    pub(crate) fn recover_maintenance(&self) -> Result<(), Error> {
        self.connection.lock().expect("database lock").execute("UPDATE context_maintenance SET status='interrupted',detail=CASE WHEN EXISTS(SELECT 1 FROM memory_operations o WHERE o.owner_id=context_maintenance.id AND o.status='committed' AND o.previous_content!=o.content) THEN 'Memory was saved, but context publication was interrupted. Start fresh context to recover without repeating the write.' WHEN EXISTS(SELECT 1 FROM memory_operations o WHERE o.owner_id=context_maintenance.id AND o.status='committed') THEN 'Memory was reviewed and unchanged, but context publication was interrupted. Start fresh context to recover the completed review without another model request.' WHEN EXISTS(SELECT 1 FROM memory_operations o WHERE o.owner_id=context_maintenance.id AND o.status='failed') THEN (SELECT json_extract(o.result,'$.detail') FROM memory_operations o WHERE o.owner_id=context_maintenance.id AND o.status='failed' LIMIT 1) || ' The old context remains active; no success marker was added.' ELSE 'Memory maintenance was interrupted. The old context remains active.' END WHERE status='running'",[])?;
        Ok(())
    }
    pub(crate) fn maintenance_status(
        &self,
        persona: &str,
    ) -> Result<Option<crate::MaintenanceStatus>, Error> {
        let mut status = self.connection.lock().expect("database lock").query_row("SELECT id,persona_id,status,detail FROM context_maintenance WHERE persona_id=?1 ORDER BY rowid DESC LIMIT 1",[persona],|r|Ok(crate::MaintenanceStatus{id:r.get(0)?,persona_id:r.get(1)?,status:r.get(2)?,detail:r.get(3)?,memory_saved:false,memory_reviewed:false})).optional()?;
        if let Some(status) = &mut status {
            status.memory_saved = self.memory_saved(&status.id)?;
            status.memory_reviewed = self.memory_reviewed(&status.id)?;
        }
        Ok(status)
    }
    pub(crate) fn context_markers(&self, room: &str) -> Result<Vec<ContextMarker>, Error> {
        let db = self.connection.lock().expect("database lock");
        let mut q=db.prepare("SELECT m.id,m.persona_id,CASE WHEN c.kind='group' THEN p.display_name || ' · fresh context' ELSE 'Fresh context' END,r.after_sequence,m.created_at FROM context_markers m JOIN context_marker_rooms r ON r.marker_id=m.id JOIN personas p ON p.id=m.persona_id JOIN conversations c ON c.id=r.conversation_id WHERE r.conversation_id=?1 ORDER BY r.after_sequence,m.created_at,m.rowid")?;
        Ok(q.query_map([room], |r| {
            Ok(ContextMarker {
                id: r.get(0)?,
                persona_id: r.get(1)?,
                label: r.get(2)?,
                after_sequence: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?)
    }
}
