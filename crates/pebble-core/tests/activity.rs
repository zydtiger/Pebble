mod classifier_support;
mod delegation_support;
mod live_support;
mod responses_support;
use live_support::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn private_output(api: &str) -> String {
    let value=json!({"state":{"affinity_user":20,"mood":"calm"},"activity":{"summary":"Quietly reviewing the saved discovery.","phase":"waiting","next_wake_minutes":30,"observed_outcomes":"$observed_outcomes","plans":null,"share_intent_id":null}}).to_string();
    if api == "responses" {
        responses_support::candidate(&value)
    } else {
        candidate(&value)
    }
}
fn eventually(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "background activity did not settle"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn heartbeat_dispatches_research_and_assimilates_without_chat_for_both_protocols() {
    for api in ["chat_completions", "responses"] {
        let server = Server::memory(vec![
            stream(delegation_support::tool(api, "fermentation-curiosity")),
            stream(private_output(api)),
            stream(if api == "responses" {
                responses_support::candidate("not JSON")
            } else {
                candidate("not JSON")
            }),
            stream(private_output(api).replace("$observed_outcomes", "incorrect acknowledgment")),
            stream(private_output(api)),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let config = root.path().join("config.toml");
        std::fs::write(
            &config,
            std::fs::read_to_string(&config)
                .unwrap()
                .replace("65536", "262144"),
        )
        .unwrap();
        let fake = tempfile::tempdir().unwrap();
        delegation_support::configure(&core, fake.path(), "gated");
        std::fs::write(fake.path().join("fake-options.json"),json!({"mode":"gated","artifact":false,"text":"A saved astronomy discovery. https://example.org/astronomy"}).to_string()).unwrap();
        let mut settings = core.activity_settings(&id).unwrap();
        settings.enabled = true;
        settings.daily_wake_budget = 2;
        settings.quiet_start_minutes = None;
        settings.quiet_end_minutes = None;
        core.save_activity_settings(&id, &settings).unwrap();
        let first = server
            .requests
            .recv_timeout(Duration::from_secs(8))
            .unwrap()
            .1;
        assert_eq!(first["tools"].as_array().unwrap().len(), 1);
        let second = server
            .requests
            .recv_timeout(Duration::from_secs(8))
            .unwrap()
            .1;
        assert!(second.to_string().contains("fermentation-curiosity"));
        eventually(|| core.persona_activity(&id).unwrap().phase == "waiting");
        let tasks = delegation_support::wait(&core, |t| {
            t.len() == 1 && t[0].state == pebble_core::DelegationState::Running
        });
        assert_eq!(
            core.persona_activity(&id).unwrap().active_task_ids,
            vec![tasks[0].id.clone()]
        );
        assert!(
            core.room_messages(&tasks[0].conversation_id)
                .unwrap()
                .is_empty()
        );
        std::fs::write(fake.path().join("release"), "").unwrap();
        let third = server
            .requests
            .recv_timeout(Duration::from_secs(8))
            .unwrap()
            .1;
        let text = third.to_string();
        assert!(text.contains("task_outcomes"));
        assert!(text.contains("A saved astronomy discovery"));
        for _ in 0..2 {
            let repair = server
                .requests
                .recv_timeout(Duration::from_secs(8))
                .unwrap()
                .1;
            assert_eq!(repair["tool_choice"], "none");
            assert!(
                repair
                    .to_string()
                    .contains("Private activity validation feedback")
            );
            assert!(repair.to_string().contains("A saved astronomy discovery"));
        }
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT wakes_used FROM persona_activity_runtime WHERE persona_id=?1",
                [&id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        eventually(|| core.delegated_tasks().unwrap()[0].report_state == "private_assimilated");
        assert!(
            core.room_messages(&tasks[0].conversation_id)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            delegation_support::method_count(fake.path(), "turn/start"),
            1
        );
        let calls = std::fs::read_to_string(fake.path().join("fake-methods.jsonl")).unwrap();
        let thread: Value = calls
            .lines()
            .map(|s| serde_json::from_str::<Value>(s).unwrap())
            .find(|v| v["method"] == "thread/start")
            .unwrap();
        assert_eq!(thread["params"]["approvalPolicy"], "on-request");
        assert_eq!(thread["params"]["approvalsReviewer"], "auto_review");
        assert_eq!(thread["params"]["config"]["web_search"], "live");
        assert_eq!(thread["params"]["sandbox"], "read-only");
        core.stop_activity(&id).unwrap();
    }
}

#[test]
fn configured_mcp_and_workspace_writes_use_existing_delegation_permissions() {
    let api = "responses";
    let server = Server::new(vec![
        stream(delegation_support::tool(api, "linguistics-curiosity")),
        stream(private_output(api)),
        stream(private_output(api)),
        stream(private_output(api)),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", api, "");
    let fake = tempfile::tempdir().unwrap();
    delegation_support::configure(&core, fake.path(), "complete");
    let mut codex = core.codex_settings().unwrap();
    codex.writable = true;
    core.save_codex_settings(&codex).unwrap();
    assert!(!core.codex_capabilities().unwrap().mcp_servers.is_empty());
    let mut settings = core.activity_settings(&id).unwrap();
    settings.enabled = true;
    settings.quiet_start_minutes = None;
    settings.quiet_end_minutes = None;
    core.save_activity_settings(&id, &settings).unwrap();
    let task = delegation_support::settled(&core);
    assert_eq!(task.state, pebble_core::DelegationState::Completed);
    let calls = std::fs::read_to_string(fake.path().join("fake-methods.jsonl")).unwrap();
    let calls: Vec<Value> = calls
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let thread = calls
        .iter()
        .find(|v| v["method"] == "thread/start")
        .unwrap();
    assert_eq!(thread["params"]["sandbox"], "workspace-write");
    assert_eq!(thread["params"]["approvalPolicy"], "on-request");
    assert_eq!(thread["params"]["approvalsReviewer"], "auto_review");
    assert_eq!(thread["params"]["config"]["web_search"], "live");
    let turn = calls.iter().find(|v| v["method"] == "turn/start").unwrap();
    let input: Value =
        serde_json::from_str(turn["params"]["input"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        input["delegated_objective"],
        "Look up synthetic evidence and save the actual result; no personal data."
    );
    assert_eq!(task.title, "Synthetic lookup");
    assert!(
        task.links
            .iter()
            .any(|link| !link.target.starts_with("https://"))
    );
    assert!(
        core.room_messages(&task.conversation_id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        delegation_support::method_count(fake.path(), "thread/start"),
        1
    );
    assert_eq!(
        delegation_support::method_count(fake.path(), "turn/start"),
        1
    );
    eventually(|| core.delegated_tasks().unwrap()[0].report_state == "private_assimilated");
    codex.writable = false;
    core.save_codex_settings(&codex).unwrap();
    core.followup_delegation(
        &task.id,
        "Continue the saved exploration with one more synthetic source.",
    )
    .unwrap();
    eventually(|| {
        delegation_support::method_count(fake.path(), "turn/start") == 2
            && core.delegated_tasks().unwrap()[0].report_state == "private_assimilated"
    });
    let calls = std::fs::read_to_string(fake.path().join("fake-methods.jsonl")).unwrap();
    let resumed: Value = calls
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|v| v["method"] == "thread/resume")
        .unwrap();
    assert_eq!(resumed["params"]["sandbox"], "workspace-write");
    assert_eq!(resumed["params"]["approvalPolicy"], "on-request");
    assert_eq!(resumed["params"]["approvalsReviewer"], "auto_review");
    assert_eq!(resumed["params"]["config"]["web_search"], "live");
    assert_eq!(core.delegated_tasks().unwrap().len(), 1);
    core.stop_activity(&id).unwrap();
}

#[test]
fn user_message_cancels_a_private_http_request_before_it_can_commit() {
    user_interruption(false);
}

#[test]
fn group_mention_cancels_a_private_http_request_before_it_can_commit() {
    user_interruption(true);
}

#[test]
fn group_input_interrupts_waiting_members_and_defers_new_private_wakes() {
    use classifier_support::{Reply, Server as ConcurrentServer};

    let first = ConcurrentServer::new(vec![
        Reply::stream(responses_support::success("First group reply")).delayed(5000),
    ]);
    let second = ConcurrentServer::new(vec![
        Reply::stream(private_output("responses")).delayed(2000),
        Reply::stream(responses_support::success("Second group reply")),
    ]);
    let (root, core, a) = isolated();
    configure(root.path(), &second.url, "mock", "responses", "");
    let b = core.create_persona("Kai").unwrap().id;
    let mut settings = core.settings(Some(&a)).unwrap();
    settings.chat.enabled = true;
    settings.chat.base_url = first.url.clone();
    settings.chat.model = "mock".into();
    settings.chat.api = "responses".into();
    settings.chat.context_len = Some(65536);
    settings.chat.max_output_tokens = Some(4096);
    core.save_settings(
        Some(&a),
        &pebble_core::SettingsUpdate {
            chat: settings.chat,
            persona: settings.persona,
            credential: pebble_core::CredentialChange::Clear,
        },
    )
    .unwrap();
    let room = core
        .create_group("Both members", &[a.clone(), b.clone()])
        .unwrap();
    let mut activity = core.activity_settings(&b).unwrap();
    activity.enabled = true;
    activity.quiet_start_minutes = None;
    activity.quiet_end_minutes = None;
    core.save_activity_settings(&b, &activity).unwrap();
    second
        .requests
        .recv_timeout(Duration::from_secs(8))
        .unwrap();
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let before: String = db
        .query_row(
            "SELECT current_state FROM persona_states WHERE persona_id=?1",
            [&b],
            |r| r.get(0),
        )
        .unwrap();
    let (submission, events) = classifier_support::submit(
        &core,
        &room.id,
        "Please both answer",
        &[a.clone(), b.clone()],
    );
    first.requests.recv_timeout(Duration::from_secs(8)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let status: String = db
            .query_row(
                "SELECT status FROM activity_calls WHERE persona_id=?1",
                [&b],
                |r| r.get(0),
            )
            .unwrap();
        if status == "cancelled" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "waiting member's private call was not cancelled: {status}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // A is still replying; B has a pending required slot, but no model_call yet.
    db.execute("UPDATE persona_activity_runtime SET next_wake_at='2000-01-01T00:00:00Z' WHERE persona_id=?1", [&b]).unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    let calls: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM activity_calls WHERE persona_id=?1",
            [&b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(calls, 1, "a pending group slot must defer new private work");
    let outputs: i64 = db.query_row("SELECT COUNT(*) FROM model_items m JOIN activity_calls c ON c.id=m.activity_call_id WHERE c.persona_id=?1 AND m.kind<>'input'", [&b], |r| r.get(0)).unwrap();
    assert_eq!(
        outputs, 0,
        "cancelled private output entered native history"
    );
    let state: String = db
        .query_row(
            "SELECT current_state FROM persona_states WHERE persona_id=?1",
            [&b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, before);
    core.stop_activity(&b).unwrap();
    let completed = classifier_support::finish(&submission, &events);
    assert_eq!(
        completed
            .iter()
            .filter(|e| e.kind == pebble_core::ChatEventKind::Completed)
            .count(),
        2
    );
}

fn user_interruption(group: bool) {
    let mut slow = stream(private_output("responses"));
    slow.delay_headers = true;
    let server = Server::memory(vec![
        stream(responses_support::candidate("not JSON")),
        slow,
        stream(responses_support::success(
            "I am here; the exploration can wait.",
        )),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "responses", "");
    let room = if group {
        let other = core.create_persona("Kai").unwrap();
        core.create_group("Interrupt exploration", &[id.clone(), other.id])
            .unwrap()
            .id
    } else {
        core.workspace().unwrap().personas[0]
            .conversation_id
            .clone()
    };
    let mut settings = core.activity_settings(&id).unwrap();
    settings.enabled = true;
    settings.quiet_start_minutes = None;
    settings.quiet_end_minutes = None;
    core.save_activity_settings(&id, &settings).unwrap();
    server
        .requests
        .recv_timeout(Duration::from_secs(8))
        .unwrap();
    let repair = server
        .requests
        .recv_timeout(Duration::from_secs(8))
        .unwrap()
        .1;
    assert!(
        repair
            .get("tools")
            .is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty))
    );
    let (_submission, events) = classifier_support::submit(
        &core,
        &room,
        "Talk to me first",
        if group {
            std::slice::from_ref(&id)
        } else {
            &[]
        },
    );
    assert_eq!(
        terminal(&events).last().unwrap().kind,
        pebble_core::ChatEventKind::Completed
    );
    let messages = core.room_messages(&room).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].content, "I am here; the exploration can wait.");
    assert!(
        core.persona_activity(&id)
            .unwrap()
            .summary
            .contains("not accepted")
    );
    core.stop_activity(&id).unwrap();
}

#[test]
fn autonomous_questions_and_permissions_stop_without_inventing_user_input() {
    for mode in ["question", "approval", "form-mixed"] {
        let server = Server::memory(vec![
            stream(delegation_support::tool("responses", "free-exploration")),
            stream(private_output("responses")),
            stream(private_output("responses")),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "responses", "");
        let fake = tempfile::tempdir().unwrap();
        delegation_support::configure(&core, fake.path(), mode);
        let mut settings = core.activity_settings(&id).unwrap();
        settings.enabled = true;
        settings.quiet_start_minutes = None;
        settings.quiet_end_minutes = None;
        core.save_activity_settings(&id, &settings).unwrap();
        let task = delegation_support::settled(&core);
        assert_eq!(
            task.state,
            pebble_core::DelegationState::Interrupted,
            "{mode}"
        );
        assert!(task.request.is_none());
        assert!(!task.detail.is_empty());
        assert!(
            core.room_messages(&task.conversation_id)
                .unwrap()
                .is_empty()
        );
        let response: Value =
            serde_json::from_slice(&std::fs::read(fake.path().join("fake-response.json")).unwrap())
                .unwrap();
        match mode {
            "question" => assert!(response.get("error").is_some()),
            "approval" => assert_eq!(response["result"]["decision"], "decline"),
            _ => assert_eq!(response["result"]["action"], "decline"),
        }
        assert_eq!(
            delegation_support::method_count(fake.path(), "turn/interrupt"),
            1
        );
        eventually(|| core.delegated_tasks().unwrap()[0].report_state == "private_assimilated");
        core.stop_activity(&id).unwrap();
    }
}

#[test]
fn exhausted_handover_repairs_keep_evidence_queued_without_repeating_research() {
    let server = Server::memory(vec![
        stream(delegation_support::tool("responses", "bounded-handover")),
        stream(private_output("responses")),
        stream(responses_support::candidate("not JSON")),
        stream(responses_support::candidate("not JSON")),
        stream(responses_support::candidate("not JSON")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "responses", "");
    let fake = tempfile::tempdir().unwrap();
    delegation_support::configure(&core, fake.path(), "gated");
    let mut settings = core.activity_settings(&id).unwrap();
    settings.enabled = true;
    settings.daily_wake_budget = 2;
    settings.quiet_start_minutes = None;
    settings.quiet_end_minutes = None;
    core.save_activity_settings(&id, &settings).unwrap();
    eventually(|| core.persona_activity(&id).unwrap().phase == "waiting");
    let tasks = delegation_support::wait(&core, |tasks| {
        tasks.len() == 1 && tasks[0].state == pebble_core::DelegationState::Running
    });
    std::fs::write(fake.path().join("release"), "").unwrap();
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    eventually(|| {
        db.query_row(
            "SELECT COUNT(*) FROM activity_calls WHERE status='failed'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
            == 1
    });
    assert_eq!(
        core.delegated_tasks().unwrap()[0].report_state,
        "private_queued"
    );
    assert_eq!(
        db.query_row(
            "SELECT wakes_used FROM persona_activity_runtime WHERE persona_id=?1",
            [&id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM activity_steps", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM activity_events WHERE kind='validation_rejected'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    assert!(
        core.room_messages(&tasks[0].conversation_id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        delegation_support::method_count(fake.path(), "turn/start"),
        1
    );
    drop(core);
    let mut reopened = None;
    eventually(|| match pebble_core::Core::open(root.path()) {
        Ok(core) => {
            reopened = Some(core);
            true
        }
        Err(pebble_core::Error::AlreadyOpen) => false,
        Err(error) => panic!("{error}"),
    });
    let core = reopened.unwrap();
    assert_eq!(
        core.delegated_tasks().unwrap()[0].report_state,
        "private_queued"
    );
    assert_eq!(
        delegation_support::method_count(fake.path(), "turn/start"),
        1
    );
}
