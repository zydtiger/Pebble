use super::*;

#[test]
fn admitted_spawn_failure_finishes_task_and_releases_owner_without_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let core = crate::Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let db = &core.database;
    let queued = db
        .enqueue_turn(&persona, "Synthetic task request.")
        .unwrap();
    let origin = db.activate_turn(&persona, &queued).unwrap();
    let request = db.request(&origin.call_id).unwrap();
    let settings = CodexSettings {
        executable: root
            .path()
            .join("never-dispatch-this-fixture")
            .display()
            .to_string(),
        ..Default::default()
    };
    let (_, id) = db
        .prepare_delegation(
            &request,
            &super::super::tool::Call {
                id: "spawn-fixture".into(),
                arguments: super::super::tool::Arguments {
                    title: "Synthetic".into(),
                    instructions: "Synthetic task".into(),
                    request_key: "shared".into(),
                    task_id: None,
                },
            },
            &[],
            &settings,
        )
        .unwrap();
    let id = id.unwrap();
    db.finish_turn(
        &origin,
        "",
        crate::MessageStatus::Failed,
        Some("fixture"),
        None,
    )
    .unwrap();
    struct Observer(Mutex<Vec<DelegationState>>);
    impl DelegationListener for Observer {
        fn on_tasks(&self, tasks: Vec<DelegatedTask>) -> Result<(), String> {
            self.0
                .lock()
                .unwrap()
                .extend(tasks.into_iter().map(|t| t.state));
            Ok(())
        }
    }
    let observer = Arc::new(Observer(Mutex::new(Vec::new())));
    core.codex.subscribe(observer.clone()).unwrap();
    core.codex.fail_next_spawn.store(true, Ordering::Release);
    assert!(core.codex.dispatch(&id).is_err());
    let task = db.task(&id).unwrap();
    assert_eq!(task.state, DelegationState::Failed);
    assert!(!task.dispatch_started && task.thread_id.is_none());
    assert!(task.detail.contains("Could not start"));
    assert!(!core.codex.commands.lock().unwrap().contains_key(&id));
    assert!(
        observer
            .0
            .lock()
            .unwrap()
            .contains(&DelegationState::Failed)
    );
    core.codex.save_settings(&settings).unwrap();
    for _ in 0..200 {
        if db.task(&id).unwrap().report_state == "completed" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(db.task(&id).unwrap().report_state, "completed");
    let released = Arc::downgrade(&core.database);
    drop(core);
    for _ in 0..100 {
        if released.strong_count() == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(released.strong_count(), 0);
    let reopened = super::recovery_tests::reopen(root.path());
    assert_eq!(
        reopened.database.task(&id).unwrap().state,
        DelegationState::Failed
    );
    reopened
        .database
        .update_task(&id, |t| {
            t.state = DelegationState::Uncertain;
            t.dispatch_started = true;
            t.thread_id = Some("synthetic-existing".into());
            t.result = "Existing partial evidence".into();
            Ok(())
        })
        .unwrap();
    reopened
        .codex
        .fail_next_spawn
        .store(true, Ordering::Release);
    assert!(reopened.codex.reconcile(&id).is_err());
    let uncertain = reopened.database.task(&id).unwrap();
    assert_eq!(uncertain.state, DelegationState::Uncertain);
    assert_eq!(uncertain.result, "Existing partial evidence");
    assert!(!reopened.codex.commands.lock().unwrap().contains_key(&id));
}

fn admitted_task() -> (tempfile::TempDir, crate::Core, String) {
    let root = tempfile::tempdir().unwrap();
    let core = crate::Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let db = &core.database;
    let queued = db
        .enqueue_turn(&persona, "Synthetic task request.")
        .unwrap();
    let origin = db.activate_turn(&persona, &queued).unwrap();
    let request = db.request(&origin.call_id).unwrap();
    let (_, id) = db
        .prepare_delegation(
            &request,
            &super::super::tool::Call {
                id: "boundary-fixture".into(),
                arguments: super::super::tool::Arguments {
                    title: "Synthetic".into(),
                    instructions: "Synthetic task".into(),
                    request_key: "shared".into(),
                    task_id: None,
                },
            },
            &[],
            &CodexSettings::default(),
        )
        .unwrap();
    db.finish_turn(
        &origin,
        "",
        crate::MessageStatus::Failed,
        Some("fixture"),
        None,
    )
    .unwrap();
    (root, core, id.unwrap())
}

#[test]
fn turn_errors_redact_authorization_and_final_frames_preserve_cancel_confirmation() {
    let (_root, core, id) = admitted_task();
    let mut task = core.database.task(&id).unwrap();
    task.state = DelegationState::Running;
    task.thread_id = Some("synthetic-thread".into());
    task.turn_id = Some("synthetic-turn".into());
    let auth = "https://example.org/authorize?state=SECRET";
    let request = json!({"id":"authorization", "method":"mcpServer/elicitation/request", "params":{
        "threadId":"synthetic-thread", "turnId":"synthetic-turn", "mode":"url", "url":auth
    }});
    events::apply(&mut task, &request, None).unwrap();
    assert!(task.request.is_some());
    let error = json!({"method":"error", "params":{
        "threadId":"synthetic-thread", "turnId":"synthetic-turn", "willRetry":true,
        "error":{"message":format!("Failed at {auth}"), "additionalDetails":"PRIVATE_STACK"}
    }});
    assert!(!events::apply(&mut task, &error, None).unwrap().terminal);
    assert_eq!(task.state, DelegationState::NeedsApproval);
    assert!(task.request.is_some());
    assert!(task.detail.contains("authorization link removed"));
    assert!(!task.detail.contains("SECRET") && !task.detail.contains("PRIVATE_STACK"));
    task.state = DelegationState::CancellationRequested;
    task.activity = "Requesting cancellation…".into();
    events::apply(&mut task, &error, None).unwrap();
    assert_eq!(task.state, DelegationState::CancellationRequested);
    assert_eq!(task.activity, "Requesting cancellation…");
    let mut fatal = error.clone();
    fatal["params"]["willRetry"] = json!(false);
    assert!(events::apply(&mut task, &fatal, None).unwrap().terminal);
    assert_eq!(task.state, DelegationState::Failed);
    assert!(task.activity.is_empty() && task.request.is_none() && task.pending_requests.is_empty());
    let final_event = json!({"method":"turn/completed", "params":{
        "threadId":"synthetic-thread", "turn":{"id":"synthetic-turn", "status":"interrupted"}
    }});
    events::apply(
        &mut task,
        &final_event,
        Some(DelegationState::CancellationRequested),
    )
    .unwrap();
    assert_eq!(task.state, DelegationState::Cancelled);
}

#[test]
fn followup_admission_clears_current_evidence_before_any_dispatch_and_archives_prior_run() {
    let (root, core, id) = admitted_task();
    core.database
        .update_task(&id, |t| {
            t.state = DelegationState::Completed;
            t.thread_id = Some("synthetic-thread".into());
            t.result = "Previous actual result".into();
            t.links = (0..32)
                .map(|i| TaskLink {
                    label: format!("Old {i}"),
                    target: format!("https://example.org/old/{i}"),
                })
                .collect();
            Ok(())
        })
        .unwrap();
    core.database
        .update_task(&id, |t| {
            t.report_state = "completed".into();
            Ok(())
        })
        .unwrap();
    let prior = core.database.task(&id).unwrap();
    core.database
        .begin_task_followup(&id, "Continue the synthetic task")
        .unwrap();
    let current = core.database.task(&id).unwrap();
    assert_eq!(current.state, DelegationState::Queued);
    assert!(!current.dispatch_started);
    assert!(current.result.is_empty() && current.links.is_empty());
    let snapshot: String = rusqlite::Connection::open(root.path().join("data/app.sqlite"))
        .unwrap()
        .query_row(
            "SELECT record FROM delegation_runs WHERE task_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot: DelegatedTask = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(snapshot.links, prior.links);
    assert_eq!(snapshot.result, prior.result);
}

#[test]
fn queued_answer_rechecks_cancelled_ownership_before_writing_to_stdio() {
    use std::os::unix::fs::PermissionsExt;
    let (root, core, id) = admitted_task();
    let script = root.path().join("fake-app-server");
    std::fs::write(
        &script,
        include_str!("../../../tests/delegation_support/app_server.py"),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let settings = CodexSettings {
        executable: script.display().to_string(),
        workspace: root.path().display().to_string(),
        ..Default::default()
    };
    let mut adapter = Adapter::start(&settings).unwrap();
    let request = TaskRequest {
        id: "\"interaction-1\"".into(),
        approval: true,
        summary: "Synthetic approval".into(),
        questions: vec![],
        authorization_url: None,
        method: "item/commandExecution/requestApproval".into(),
        payload: Value::Null,
    };
    core.database
        .update_task(&id, |t| {
            t.state = DelegationState::NeedsApproval;
            t.request = Some(request.clone());
            t.pending_requests.push(request.clone());
            Ok(())
        })
        .unwrap();
    // A response admitted earlier can still be in the worker command queue at Cancel.
    let command = Command::Respond {
        request: request.id.clone(),
        result: json!({"decision":"accept"}),
    };
    core.codex.cancel(&id).unwrap();
    if let Command::Respond { request, result } = command {
        core.codex
            .send_answer(&id, &request, result, &mut adapter)
            .unwrap();
    }
    // The RPC is a wire-order barrier: any accidentally sent approval was consumed first.
    adapter.rpc("account/read", json!({})).unwrap();
    assert!(!root.path().join("fake-response.json").exists());
    assert!(!root.path().join("fake-responses.jsonl").exists());
    let task = core.database.task(&id).unwrap();
    assert_eq!(task.state, DelegationState::CancellationRequested);
    assert!(task.request.is_none() && task.pending_requests.is_empty());
}

#[test]
fn task_description_uses_first_public_commentary_once_per_run_and_never_terminal_evidence() {
    let (_root, core, id) = admitted_task();
    let mut task = core.database.task(&id).unwrap();
    task.state = DelegationState::Running;
    task.thread_id = Some("summary-thread".into());
    task.turn_id = Some("summary-turn".into());
    let commentary = |text: &str| json!({"method":"item/completed","params":{"threadId":"summary-thread","turnId":"summary-turn","item":{"type":"agentMessage","phase":"commentary","text":text}}});
    events::apply(&mut task, &commentary(" \n\t"), None).unwrap();
    assert_eq!(task.title, "Synthetic");
    events::apply(
        &mut task,
        &commentary("  Inspect   the release\nnotes "),
        None,
    )
    .unwrap();
    assert_eq!(task.title, "Inspect the release notes");
    assert!(task.result.is_empty() && task.links.is_empty());
    events::apply(&mut task, &commentary("Later progress"), None).unwrap();
    assert_eq!(task.title, "Inspect the release notes");
    let encoded = serde_json::to_string(&task).unwrap();
    let mut reloaded: DelegatedTask = serde_json::from_str(&encoded).unwrap();
    events::apply(&mut reloaded, &commentary("Resumed progress"), None).unwrap();
    assert_eq!(reloaded.title, task.title);
    task.summary_captured = false;
    events::apply(&mut task, &commentary(&"界".repeat(120)), None).unwrap();
    assert_eq!(task.title.chars().count(), 96);
    task.summary_captured = false;
    let title = task.title.clone();
    events::apply(&mut task, &commentary(&"x".repeat(161)), None).unwrap();
    events::apply(&mut task, &commentary("Unrelated later update"), None).unwrap();
    assert_eq!(task.title, title);
    task.summary_captured = false;
    task.state = DelegationState::Failed;
    events::apply(
        &mut task,
        &commentary("Late commentary"),
        Some(DelegationState::Running),
    )
    .unwrap();
    assert_eq!(task.title, title);
}

#[test]
fn deferred_clarification_remains_blocked_through_completed_frames_and_saved_thread_recovery() {
    let (_root, core, id) = admitted_task();
    let mut task = core.database.task(&id).unwrap();
    task.state = DelegationState::Running;
    task.thread_id = Some("deferred-thread".into());
    task.turn_id = Some("deferred-turn".into());
    let question = json!({"id":"question","method":"item/tool/requestUserInput","params":{"threadId":"deferred-thread","turnId":"deferred-turn","questions":[{"id":"scope","header":"Scope","question":"Which file?","options":[{"label":"README","description":"Read documentation only"}]}]}});
    events::apply(&mut task, &question, None).unwrap();
    assert!(events::defer_interaction(&mut task).is_some());
    assert!(task.request.is_none());
    let evidence = json!({"method":"item/completed","params":{"threadId":"deferred-thread","turnId":"deferred-turn","item":{"type":"agentMessage","phase":"final_answer","text":"Partial evidence \u{e200}unresolved\u{e201}"}}});
    events::apply(&mut task, &evidence, None).unwrap();
    let completed = json!({"method":"turn/completed","params":{"threadId":"deferred-thread","turn":{"id":"deferred-turn","status":"completed"}}});
    events::apply(&mut task, &completed, None).unwrap();
    assert_eq!(task.state, DelegationState::Failed);
    assert!(task.detail.contains("Which file?") && task.detail.contains("Read documentation only"));
    let mut task: DelegatedTask =
        serde_json::from_str(&serde_json::to_string(&task).unwrap()).unwrap();
    events::reconcile(
        &mut task,
        &json!({"thread":{"turns":[{"id":"deferred-turn","status":"completed","items":[]}]}}),
    )
    .unwrap();
    assert_eq!(task.state, DelegationState::Failed);
    assert!(task.detail.contains("Which file?"));
    assert!(!task.detail.contains("Recovered the completed result"));
}
