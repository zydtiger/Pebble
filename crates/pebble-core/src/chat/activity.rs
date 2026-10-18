use super::*;

impl Chats {
    pub(crate) fn start_private_activity(
        self: &Arc<Self>,
        db: Arc<Database>,
        persona: Persona,
        step: String,
    ) -> Result<(), Error> {
        let mut queues = self.queues.lock().expect("generation queues lock");
        let queue = queues.entry(persona.id.clone()).or_default().clone();
        let control = Arc::new(Control::default());
        let reservation = queue.reserve(&format!("activity:{step}"), &control);
        drop(queues);
        let chats = Arc::downgrade(self);
        let worker_db = db.clone();
        let worker_persona = persona.clone();
        let worker_step = step.clone();
        let spawned=std::thread::Builder::new().name("pebble-private-activity".into()).spawn(move || {
            if !reservation.wait(&control) {let _=worker_db.finish_quiet_activity_step(&worker_persona.id,&worker_step);return}
            if let Some(chats)=chats.upgrade() {chats.maintain_with_control(&worker_db,&worker_persona,None,Some(&control));}
            let Some(chat)=worker_persona.chat.as_ref() else {let _=worker_db.finish_quiet_activity_step(&worker_persona.id,&worker_step);return};
            if control.is_cancelled() {let _=worker_db.finish_quiet_activity_step(&worker_persona.id,&worker_step);return}
            let request=match worker_db.activate_activity_call(&worker_persona,&worker_step) {
                Ok(request)=>request,
                Err(error)=>{let _=worker_db.fail_pending_activity(&worker_persona.id,&worker_step,&error.to_string());return}
            };
            let output=private_activity_generate(&worker_db,&worker_persona,chat,&request,&control);
            let accepted=match output {
                Ok(output)=>control.while_active(||worker_db.finish_activity_call(&request,"completed",None,Some(&output))).unwrap_or_else(||worker_db.finish_activity_call(&request,"cancelled",None,None)),
                Err(error)=>worker_db.finish_activity_call(&request,if control.is_cancelled(){"cancelled"}else{"failed"},Some(&error.to_string()),None),
            };
            if let Err(error)=accepted {let _=worker_db.finish_activity_call(&request,"failed",Some(&error.to_string()),None);}
            if !control.is_cancelled() && let Ok(Some(share))=worker_db.begin_activity_share(&worker_persona,&request.call_id) {
                let output=worker_runtime().and_then(|runtime|runtime.block_on(async {tokio::select! {
                    biased;
                    _=control.cancelled()=>Err(Error::Generation("Cancelled".into())),
                    result=crate::live::generate(chat,&share,worker_persona.context.max_output_tokens,control.clone(), |_|Ok(()))=>result,
                }}));
                let result=match output {
                    Ok(output)=>control.while_active(||worker_db.finish_activity_share(&share,Some(&output),None)).unwrap_or_else(||worker_db.finish_activity_share(&share,None,Some("Cancelled"))),
                    Err(error)=>worker_db.finish_activity_share(&share,None,Some(&error.to_string())),
                };
                if let Err(error)=result {let _=worker_db.finish_activity_share(&share,None,Some(&error.to_string()));}
            }
            if let Some(chats)=chats.upgrade() {chats.maintain_with_control(&worker_db,&worker_persona,None,Some(&control));}
        });
        if spawned.is_err() {
            db.fail_pending_activity(
                &persona.id,
                &step,
                "Could not start private activity worker.",
            )?;
            return Err(Error::Generation(
                "Could not start private activity worker.".into(),
            ));
        }
        Ok(())
    }
}

/// A private call can accept at most one Rust-admitted exploration task,
/// then must produce its final no-reply decision. Native call/result items are
/// committed before dispatch, so cancellation can only suppress the final
/// decision, never erase accepted task effects.
fn private_activity_generate(
    db: &Arc<Database>,
    persona: &Persona,
    chat: &crate::ChatConnection,
    request: &crate::ModelRequest,
    control: &Arc<Control>,
) -> Result<Vec<serde_json::Value>, Error> {
    let runtime = worker_runtime()?;
    let mut candidate = request.clone();
    let mut invalid = 0usize;
    let mut finals = 0usize;
    let mut feedback: Option<String> = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(Error::Generation(
                "The complete private activity operation exceeded 180 seconds.".into(),
            ));
        }
        let mut attempt = candidate.clone();
        if let Some(detail) = &feedback {
            let note = format!(
                "Private activity validation feedback: {detail} Regenerate the complete private JSON from the SAME frozen input, current_state and task_outcomes. Return no reply. Acknowledge each exact task_id/outcome_revision in observed_outcomes. No rejected state or plan was accepted; do not repeat tools or research."
            );
            crate::memory::append(
                &mut attempt,
                &[crate::storage::input_message(
                    &request.protocol,
                    "user",
                    &note,
                )],
            )?;
        }
        let body = crate::live::request_body(chat, &attempt, persona.context.max_output_tokens)?.1;
        if candidate.delegation_tools {
            persona.context.check_chat_start(&body)?;
        } else {
            persona.context.check_body(&body)?;
        }
        let output=runtime.block_on(async { tokio::select! {
            biased;
            _=control.cancelled()=>Err(Error::Generation("Cancelled".into())),
            result=tokio::time::timeout(deadline.saturating_duration_since(std::time::Instant::now()),crate::live::generate(chat,&attempt,persona.context.max_output_tokens,control.clone(), |_|Ok(())))=>result.map_err(|_|Error::Generation("The complete private activity operation exceeded 180 seconds.".into()))?,
        } })?;
        if control.is_cancelled() {
            return Err(Error::Generation("Cancelled".into()));
        }
        match crate::codex::tool::calls(&output)? {
            crate::codex::tool::ParsedCall::None => {
                finals += 1;
                match db.private_activity_feedback(request, &output)? {
                    None => return Ok(output),
                    Some(detail) => {
                        db.record_activity_validation(request, &detail)?;
                        if finals == 3 {
                            return Err(Error::Generation(format!(
                                "Private activity validation failed after 3 candidates: {detail}"
                            )));
                        }
                        candidate.delegation_tools = false;
                        feedback = Some(detail);
                    }
                }
            }
            crate::codex::tool::ParsedCall::InvalidArguments { id, detail } => {
                if !candidate.delegation_tools {
                    return Err(crate::codex::failure(
                        "Tools are unavailable while finalizing private activity.",
                    ));
                }
                invalid += 1;
                let result = crate::codex::tool::argument_feedback(
                    &candidate.protocol,
                    &id,
                    &detail,
                    3 - invalid,
                );
                control
                    .while_active(|| db.record_activity_tool_exchange(&candidate, &output, &result))
                    .ok_or_else(|| Error::Generation("Cancelled".into()))??;
                if invalid == 3 {
                    return Err(crate::codex::failure(&format!(
                        "Invalid private delegate_task arguments after 3 attempts: {detail}"
                    )));
                }
                candidate = db.activity_followup_request(&candidate)?;
                candidate.delegation_tools = true;
            }
            crate::codex::tool::ParsedCall::Valid(call) => {
                if !candidate.delegation_tools {
                    return Err(crate::codex::failure(
                        "This private call already used its one research operation.",
                    ));
                }
                let manager = db
                    .codex
                    .lock()
                    .expect("task manager")
                    .upgrade()
                    .ok_or_else(|| crate::codex::failure("Task manager is closing."))?;
                let (result, dispatch) = control
                    .while_active(|| {
                        db.prepare_activity_delegation(
                            &candidate,
                            &call,
                            &output,
                            &manager.settings()?,
                        )
                    })
                    .ok_or_else(|| Error::Generation("Cancelled".into()))??;
                if let Some(id) = dispatch {
                    manager.dispatch(&id)?;
                }
                manager.notify();
                candidate = db.activity_followup_request(&candidate)?;
                candidate.delegation_tools = false;
                let _ = result;
            }
        }
    }
}
