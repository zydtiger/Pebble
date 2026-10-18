use super::*;

impl Database {
    /// Store the persona-selected objective with normal frozen Codex settings
    /// and private delivery before dispatch. No conversation transcript is copied.
    pub(crate) fn prepare_activity_delegation(
        &self,
        request: &crate::ModelRequest,
        call: &crate::codex::tool::Call,
        output: &[Value],
        settings: &crate::CodexSettings,
    ) -> Result<(Value, Option<String>), Error> {
        if call.arguments.task_id.is_some() {
            return Err(crate::codex::failure(
                "Private exploration starts new tasks only; an actual user reply is required to continue a task needing input.",
            ));
        }
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        if let Some(result)=tx.query_row("SELECT result FROM activity_delegation_submissions WHERE owner_id=?1 AND provider_call_id=?2",params![request.call_id,call.id],|r|r.get::<_,String>(0)).optional()? { return Ok((serde_json::from_str(&result)?,None)); }
        let (persona, activity, step, status): (String, String, String, String) = tx.query_row(
            "SELECT persona_id,activity_id,step_id,status FROM activity_calls WHERE id=?1",
            [&request.call_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if status != "streaming" {
            return Err(crate::codex::failure(
                "The private activity call is no longer active.",
            ));
        }
        let active = policy::read(&self.root, &persona)?.active();
        if !active {
            return Err(crate::codex::failure(
                "Private activity was disabled or stopped before task admission.",
            ));
        }
        if scheduler::user_work_pending(&tx, &persona)? {
            return Err(Error::Busy);
        }
        let mut dispatch = None;
        let outcome=if !settings.enabled {
            json!({"ok":false,"detail":"Codex is disabled. No private research task was accepted."})
        } else if let Some(id)=tx.query_row("SELECT id FROM delegated_tasks WHERE activity_step_id=?1 OR (activity_id=?2 AND json_extract(record,'$.state') IN ('queued','running','needs_input','needs_approval','cancellation_requested')) ORDER BY rowid DESC LIMIT 1",params![step,activity],|r|r.get::<_,String>(0)).optional()? {
            json!({"ok":true,"task_id":id,"state":"existing","detail":"This private activity already has an accepted research task for this step or one still running."})
        } else {
            let id=super::super::turns::new_id(); let created:String=tx.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')",[],|r|r.get(0))?;
            let conversation:String=tx.query_row("SELECT id FROM conversations WHERE persona_id=?1 AND kind='direct'",[&persona],|r|r.get(0))?;
            let title = &call.arguments.title;
            let instructions = &call.arguments.instructions;
            let original_request = "Pebble private activity is enabled by the user. Explore the character-selected objective using the configured Codex delegation permissions, available MCP tools and the independent task workspace. This is background exploration, not a new user message. It does not authorize external messages, purchases, deletion, or unrelated private-data disclosure.";
            let task=DelegatedTask { id:id.clone(),title:title.clone(),summary_captured:false,interaction_deferred:false,persona_id:persona.clone(),conversation_id:conversation,state:DelegationState::Queued,activity:"Waiting to start background exploration…".into(),detail:String::new(),local_limitation:String::new(),result:String::new(),links:Vec::new(),request:None,pending_requests:Vec::new(),resolved_requests:Vec::new(),authorization_url_digests:Vec::new(),report_state:"pending".into(),thread_id:None,turn_id:None,created_at:created,settings:crate::codex::settings::TaskSettings::new(settings,&self.root.join("data/codex-workspaces").join(&id)),trigger:format!("activity:{step}"),membership:None,instructions:instructions.clone(),original_request:original_request.into(),followup_request:String::new(),request_key:"private_exploration".into(),generation:1,report_call:None,outcome_revision:0,reports:Vec::new(),submission:super::super::turns::new_id(),dispatch_started:false,approval_previews:Default::default(),origin:"autonomous".into(),delivery:"private_observation".into(),activity_id:Some(activity.clone()),activity_step_id:Some(step.clone()) };
            tx.execute("INSERT INTO delegated_tasks(id,conversation_id,persona_id,trigger_id,request_key,record,origin,delivery,activity_id,activity_step_id) VALUES(?1,?2,?3,?4,'private_exploration',?5,'autonomous','private_observation',?6,?7)",params![id,task.conversation_id,task.persona_id,task.trigger,serde_json::to_string(&task)?,activity,step])?;
            tx.execute("UPDATE activity_steps SET task_id=?2 WHERE id=?1",params![step,id])?;
            tx.execute("UPDATE persona_activities SET phase='exploring',progress='Background exploration is running.',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",[&activity])?;
            dispatch=Some(id.clone());
            json!({"ok":true,"task_id":id,"state":"accepted","detail":"A background exploration task was accepted. Its saved outcome returns only to this private activity."})
        };
        let result = crate::memory::result(&request.protocol, &call.id, &outcome.to_string());
        for item in output {
            insert_activity_item(&tx, &request.session_id, &request.call_id, item)?;
        }
        insert_activity_item(&tx, &request.session_id, &request.call_id, &result)?;
        tx.execute("UPDATE activity_calls SET input_last_sequence=(SELECT MAX(sequence) FROM model_items WHERE session_id=?2) WHERE id=?1",params![request.call_id,request.session_id])?;
        tx.execute("INSERT INTO activity_delegation_submissions(owner_id,provider_call_id,task_id,result) VALUES(?1,?2,?3,?4)",params![request.call_id,call.id,outcome["task_id"].as_str(),serde_json::to_string(&result)?])?;
        tx.commit()?;
        Ok((result, dispatch))
    }

    pub(crate) fn record_activity_tool_exchange(
        &self,
        request: &crate::ModelRequest,
        output: &[Value],
        result: &Value,
    ) -> Result<(), Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        let (active, persona): (bool, String) = tx.query_row(
            "SELECT status='streaming',persona_id FROM activity_calls WHERE id=?1",
            [&request.call_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if !active {
            return Err(crate::codex::failure(
                "The private activity call is no longer active.",
            ));
        }
        if scheduler::user_work_pending(&tx, &persona)? {
            return Err(Error::Busy);
        }
        for item in output {
            insert_activity_item(&tx, &request.session_id, &request.call_id, item)?;
        }
        insert_activity_item(&tx, &request.session_id, &request.call_id, result)?;
        tx.execute("UPDATE activity_calls SET input_last_sequence=(SELECT MAX(sequence) FROM model_items WHERE session_id=?2) WHERE id=?1",params![request.call_id,request.session_id])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn activity_followup_request(
        &self,
        request: &crate::ModelRequest,
    ) -> Result<crate::ModelRequest, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        let (schema,state):(String,String)=tx.query_row("SELECT s.response_schema,c.state_before FROM activity_calls c JOIN model_sessions s ON s.id=c.session_id WHERE c.id=?1 AND c.status='streaming'",[&request.call_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let schema = crate::structured::private_schema(&serde_json::from_str::<Value>(&schema)?)
            .map_err(Error::Generation)?;
        let next = activity_request(
            &tx,
            &self.root,
            &request.call_id,
            &schema,
            serde_json::from_str(&state)?,
            false,
        )?;
        tx.commit()?;
        Ok(next)
    }

    /// A terminal autonomous task becomes private evidence and wakes the same
    /// persona queue. It never creates a report turn, room event, bubble, or
    /// notification.
    pub(crate) fn queue_activity_task_outcome(&self, id: &str) -> Result<bool, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        let record: String = tx.query_row(
            "SELECT record FROM delegated_tasks WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        let mut task: DelegatedTask = serde_json::from_str(&record)?;
        if task.delivery != "private_observation"
            || task.report_state != "pending"
            || task.state.active()
        {
            tx.commit()?;
            return Ok(false);
        }
        let activity = task.activity_id.clone().ok_or_else(|| {
            Error::Generation("Private task is missing its activity identity.".into())
        })?;
        task.report_state = "private_queued".into();
        tx.execute(
            "UPDATE delegated_tasks SET record=?2 WHERE id=?1",
            params![id, serde_json::to_string(&task)?],
        )?;
        let resumable:bool=policy::read(&self.root, &task.persona_id)?.active() && tx.query_row("SELECT EXISTS(SELECT 1 FROM persona_activities WHERE id=?1 AND phase NOT IN ('stopped','failed'))",[&activity],|r|r.get(0))?;
        if resumable {
            tx.execute("UPDATE persona_activities SET phase='assimilating',progress='Research outcome is ready for private review.',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",[&activity])?;
            tx.execute("UPDATE persona_activity_runtime SET next_wake_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'),next_reason='Private research outcome ready',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE persona_id=?1",[&task.persona_id])?;
        }
        tx.execute("INSERT INTO activity_events(id,activity_id,ordinal,kind,summary,provenance) VALUES(?1,?2,(SELECT COALESCE(MAX(ordinal),0)+1 FROM activity_events WHERE activity_id=?2),'task_outcome','Private research outcome saved','codex')",params![Uuid::new_v4().to_string(),activity])?;
        tx.commit()?;
        Ok(true)
    }
}
