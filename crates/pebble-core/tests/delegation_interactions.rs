mod delegation_support;
mod live_support;
mod responses_support;
use delegation_support::{
    assert_fake, configure as codex_configure, method_count, settled, tool, wait,
};
use live_support::*;
use pebble_core::{DelegationState, TaskAnswer};
use serde_json::{Value, json};

#[test]
fn report_configuration_failure_finishes_pending_call_and_retries_without_restart_or_work() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "gated-report")),
        stream(success("Started separately.")),
        stream(success(
            "Here is the saved evidence after configuration repair.",
        )),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "gated");
    assert_fake(&core, workspace.path());
    let persona = core.workspace().unwrap().personas.remove(0);
    let config = std::path::Path::new(&persona.directory).join("config.toml");
    let saved = std::fs::read(&config).unwrap();
    let (_handle, events) = start(&core, &id, "Look up synthetic evidence.");
    terminal(&events);
    wait(&core, |t| t.len() == 1 && t[0].turn_id.is_some());
    std::fs::write(&config, "[invalid TOML").unwrap();
    std::fs::write(workspace.path().join("release"), "release").unwrap();
    let task = settled(&core);
    assert_eq!(task.state, DelegationState::Completed);
    assert_eq!(task.report_state, "failed");
    assert!(task.result.contains("Verified synthetic result"));
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let (call, bubble, error): (String, String, String) = db.query_row(
        "SELECT c.status,m.status,c.error FROM delegated_tasks t JOIN model_calls c ON c.id=json_extract(t.record,'$.report_call') JOIN messages m ON m.id=c.reply_message_id WHERE t.id=?1", [&task.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    ).unwrap();
    assert_eq!((call.as_str(), bubble.as_str()), ("failed", "failed"));
    assert!(error.contains("config.toml"));
    std::fs::write(config, saved).unwrap();
    core.retry_delegation_report(&task.id).unwrap();
    wait(&core, |t| t[0].report_state == "completed");
    let request = (0..3)
        .map(|_| {
            server
                .requests
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap()
                .1
        })
        .collect::<Vec<_>>()
        .pop()
        .unwrap();
    assert_eq!(request["tool_choice"], "none");
    assert!(request.to_string().contains("Verified synthetic result"));
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    assert!(core.retry_delegation_report(&task.id).is_err());
}

#[test]
fn mcp_choices_preserve_labels_and_submit_const_strings_and_arrays_validated_by_schema() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "choices")),
        stream(success("Started.")),
        stream(success("Saved choice result.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "form-choices");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &id, "Answer the synthetic form.");
    terminal(&events);
    let task = wait(&core, |t| t[0].request.is_some()).remove(0);
    let request = task.request.unwrap();
    let single = request.questions.iter().find(|q| q.id == "format").unwrap();
    assert!(!single.multiple);
    assert_eq!(
        (&single.choices[0].label[..], &single.choices[0].value[..]),
        ("Markdown document", "md")
    );
    let titled = request.questions.iter().find(|q| q.id == "titled").unwrap();
    assert!(titled.multiple);
    assert_eq!(
        (&titled.choices[0].label[..], &titled.choices[0].value[..]),
        ("Source files", "src")
    );
    let answer = |id: &str, values: &[&str]| TaskAnswer {
        question_id: id.into(),
        answers: values.iter().map(|v| (*v).into()).collect(),
    };
    let mut answers = vec![
        answer("format", &["Markdown document"]),
        answer("untitled", &["red", "blue"]),
        answer("titled", &["src", "docs"]),
        answer("optional", &[]),
    ];
    assert!(
        core.answer_delegation(&task.id, &request.id, None, &answers)
            .is_err()
    );
    answers[0] = answer("format", &["md"]);
    answers[1] = answer("untitled", &[]);
    assert!(
        core.answer_delegation(&task.id, &request.id, None, &answers)
            .is_err()
    );
    answers[1] = answer("untitled", &["red", "blue"]);
    core.answer_delegation(&task.id, &request.id, None, &answers)
        .unwrap();
    assert_eq!(settled(&core).state, DelegationState::Completed);
    let response: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        response["result"]["content"],
        json!({"format":"md","untitled":["red","blue"],"titled":["src","docs"]})
    );
}

#[test]
fn unsupported_nested_mcp_form_is_visible_and_never_sends_an_answer() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "unsupported")),
        stream(success("Started.")),
        stream(success("The form was unsupported.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "form-unsupported");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &id, "Answer a synthetic form.");
    terminal(&events);
    let task = wait(&core, |t| t[0].detail.contains("form is unsupported")).remove(0);
    assert!(task.request.is_none());
    core.cancel_delegation(&task.id).unwrap();
    settled(&core);
    let response: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
    )
    .unwrap();
    assert!(response.get("error").is_some());
    assert!(response.get("result").is_none());
}

#[test]
fn command_approvals_show_parsed_actions_and_identify_stdin_without_invented_text() {
    for mode in ["approval-actions", "approval-stdin"] {
        let server = Server::memory(vec![
            stream(tool("chat_completions", mode)),
            stream(success("Started.")),
            stream(success("Denied scoped operation.")),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), mode);
        assert_fake(&core, workspace.path());
        let (_handle, events) = start(&core, &id, "Review the requested synthetic operation.");
        terminal(&events);
        let task = wait(&core, |t| t[0].request.is_some()).remove(0);
        let request = task.request.unwrap();
        if mode == "approval-actions" {
            for detail in [
                "Read file",
                "Name: README.md",
                "Path: README.md",
                "Command: cat README.md",
                "Search",
                "Pattern: TODO",
                "Path: src",
                "List files",
                "Path: docs",
            ] {
                assert!(request.summary.contains(detail), "{}", request.summary);
            }
        } else {
            assert!(
                request
                    .summary
                    .contains("Send input to an existing terminal")
            );
            assert!(
                request
                    .summary
                    .contains("did not supply operation text or parsed action details")
            );
            assert!(!request.summary.contains("Execute a command"));
        }
        assert!(!request.summary.contains("Run the requested command"));
        core.answer_delegation(&task.id, &request.id, Some(false), &[])
            .unwrap();
        settled(&core);
        let response: Value = serde_json::from_slice(
            &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(response["result"]["decision"], "decline");
    }
}

#[test]
fn oversized_approval_scope_is_explicitly_refused_instead_of_partially_authorized() {
    for mode in [
        "oversized-permissions",
        "oversized-command",
        "oversized-file",
        "oversized-reason",
        "oversized-network",
    ] {
        let server = Server::memory(vec![
            stream(tool("chat_completions", mode)),
            stream(success("Started.")),
            stream(success("The oversized operation was not authorized.")),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), mode);
        assert_fake(&core, workspace.path());
        let (_handle, events) = start(
            &core,
            &id,
            "Review the complete synthetic operation before allowing it.",
        );
        terminal(&events);
        let task = wait(&core, |t| t[0].detail.contains("full scope exceeds")).remove(0);
        assert!(
            task.request.is_none(),
            "An oversized approval must never expose Allow once"
        );
        assert!(
            core.answer_delegation(&task.id, "\"interaction-1\"", Some(true), &[])
                .is_err()
        );
        core.cancel_delegation(&task.id).unwrap();
        settled(&core);
        let response: Value = serde_json::from_slice(
            &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
        )
        .unwrap();
        assert!(response.get("error").is_some());
        assert!(response.get("result").is_none());
        if mode == "oversized-file" {
            let record: String = rusqlite::Connection::open(root.path().join("data/app.sqlite"))
                .unwrap()
                .query_row(
                    "SELECT record FROM delegated_tasks WHERE id=?1",
                    [&task.id],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(
                record.contains("last actual change"),
                "The proposed file preview itself must not silently truncate"
            );
        }
        assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    }
}

#[test]
fn authorization_url_is_request_only_and_cleared_before_result_report_and_reload() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "url")),
        stream(success("Started.")),
        stream(success("The actual connected-tool result is saved.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "url");
    std::fs::write(workspace.path().join("fake-options.json"), json!({"mode":"url", "url":"hTtPs://example.org/authorize?state=secret!", "web_url":"hTtPs://example.org/authorize?state=secret!", "text":"Authorized with hTtPs://example.org/authorize?state=secret! . Actual source: https://example.org/evidence", "status":"failed", "runtime_error":"Synthetic tool error at hTtPs://example.org/authorize?state=secret! . Repair the tool."}).to_string()).unwrap();
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(
        &core,
        &id,
        "Complete the synthetic connected-tool authorization.",
    );
    terminal(&events);
    let task = wait(&core, |t| t[0].request.is_some()).remove(0);
    let request = task.request.unwrap();
    assert_eq!(
        request.authorization_url.as_deref(),
        Some("hTtPs://example.org/authorize?state=secret!")
    );
    assert!(task.links.is_empty());
    core.answer_delegation(&task.id, &request.id, Some(true), &[])
        .unwrap();
    let resolved = wait(&core, |t| {
        t[0].request.is_none() && t[0].state == DelegationState::Running
    })
    .remove(0);
    assert!(
        !serde_json::to_string(&resolved)
            .unwrap()
            .contains("state=secret")
    );
    std::fs::write(workspace.path().join("release"), "release").unwrap();
    let completed = settled(&core);
    assert_eq!(completed.report_state, "completed");
    assert_eq!(completed.state, DelegationState::Failed);
    assert!(
        completed
            .detail
            .contains("Synthetic tool error at [authorization link removed]")
    );
    assert!(completed.result.contains("[authorization link removed]"));
    assert_eq!(
        completed
            .links
            .iter()
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        vec!["https://example.org/evidence"]
    );
    assert!(
        !serde_json::to_string(&completed)
            .unwrap()
            .contains("state=secret")
    );
    for _ in 0..3 {
        let body = server
            .requests
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .1
            .to_string();
        assert!(!body.contains("state=secret") && !body.contains("/authorize"));
    }
    drop(core);
    let reopened = pebble_core::Core::open(root.path()).unwrap();
    assert!(
        !serde_json::to_string(&reopened.delegated_tasks().unwrap())
            .unwrap()
            .contains("state=secret")
    );
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
}

#[test]
fn non_http_authorization_url_is_unsupported_and_never_becomes_evidence() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "url-invalid")),
        stream(success("Started.")),
        stream(success("No authorization was granted.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "url-invalid");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &id, "Review a synthetic authorization link.");
    terminal(&events);
    let task = wait(&core, |t| {
        t[0].detail.contains("unsupported authorization URL")
    })
    .remove(0);
    assert!(task.request.is_none() && task.links.is_empty());
    core.cancel_delegation(&task.id).unwrap();
    settled(&core);
    let response: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
    )
    .unwrap();
    assert!(response.get("error").is_some() && response.get("result").is_none());
}

#[test]
fn terminal_task_followup_api_waits_for_pending_and_queued_explanations() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "eligibility")),
        stream(success("Started.")),
        stream(success("Saved evidence.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "complete");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &id, "Look up synthetic evidence.");
    terminal(&events);
    let task = settled(&core);
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    for state in ["pending", "queued"] {
        // Freeze each supported report-wait state without racing its automatic admission.
        db.execute(
            "UPDATE delegated_tasks SET record=json_set(record,'$.report_state',?2) WHERE id=?1",
            rusqlite::params![task.id, state],
        )
        .unwrap();
        let error = core
            .followup_delegation(&task.id, "Continue this same task.")
            .unwrap_err();
        assert!(error.to_string().contains("explanation before a follow-up"));
        let current = core.delegated_tasks().unwrap().remove(0);
        assert_eq!(current.state, DelegationState::Completed);
        assert_eq!(current.report_state, state);
        assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    }
}

#[test]
fn acknowledged_interrupt_without_terminal_has_bounded_uncertainty_and_reconciles_without_work() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "cancel-ack-only")),
        stream(success("Started.")),
        stream(success("Cancellation outcome is uncertain.")),
        stream(success("The actual recovered result.")),
        stream(success("The final confirmed result.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "cancel-ack-only");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &id, "Execute synthetic scoped work.");
    terminal(&events);
    let task = wait(&core, |t| t[0].turn_id.is_some()).remove(0);
    let started = std::time::Instant::now();
    core.cancel_delegation(&task.id).unwrap();
    let uncertain = settled(&core);
    assert!(started.elapsed() < std::time::Duration::from_secs(9));
    assert_eq!(uncertain.state, DelegationState::Uncertain);
    assert!(uncertain.detail.contains("did not confirm its outcome"));
    assert!(uncertain.request.is_none() && uncertain.activity.is_empty());
    core.save_codex_settings(&core.codex_settings().unwrap())
        .unwrap();
    assert_eq!(method_count(workspace.path(), "turn/interrupt"), 1);
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    drop(core);
    let core = pebble_core::Core::open(root.path()).unwrap();
    wait(&core, |t| t[0].result.contains("Saved partial effects"));
    let recovered = core.delegated_tasks().unwrap().remove(0);
    assert_eq!(recovered.state, DelegationState::Uncertain);
    let thread = recovered.thread_id.unwrap();
    std::fs::write(workspace.path().join(format!("fake-saved-{thread}.json")), json!({"id":recovered.turn_id,"status":"completed","items":[{"id":"final","type":"agentMessage","phase":"final_answer","text":"Actual confirmed result after reconciliation."}]}).to_string()).unwrap();
    // Recovery relinquishes the execution owner before an explicit second reconciliation.
    for _ in 0..100 {
        if core.reconcile_delegation(&task.id).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wait(&core, |t| {
        t[0].state == DelegationState::Completed && t[0].report_state == "completed"
    });
    assert!(
        !core.delegated_tasks().unwrap()[0]
            .detail
            .contains("did not confirm")
    );
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
}

#[test]
fn unsupported_request_and_runtime_error_survive_failed_completion_report_and_recovery() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "form-failed")),
        stream(success("Started.")),
        stream(success(
            "The local form was refused and the tool needs repair.",
        )),
        stream(success("Recovered failure details.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "form-failed");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &id, "Use the synthetic connected tool.");
    terminal(&events);
    let task = settled(&core);
    assert_eq!(task.state, DelegationState::Failed);
    assert!(
        task.detail.contains("form is unsupported")
            && task.detail.contains("repair its local configuration")
    );
    assert!(!task.detail.contains("PRIVATE_RAW_STACK"));
    assert!(task.detail.chars().count() < 1500);
    let bodies = (0..3)
        .map(|_| {
            server
                .requests
                .recv_timeout(std::time::Duration::from_secs(3))
                .unwrap()
                .1
        })
        .collect::<Vec<_>>();
    assert!(bodies[2].to_string().contains("form is unsupported"));
    assert!(
        bodies[2]
            .to_string()
            .contains("repair its local configuration")
    );
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    db.execute("UPDATE delegated_tasks SET record=json_set(record,'$.state','interrupted','$.detail','Generic old unknown outcome') WHERE id=?1", [&task.id]).unwrap();
    drop(core);
    let reopened = pebble_core::Core::open(root.path()).unwrap();
    let recovered = wait(&reopened, |t| t[0].state == DelegationState::Failed).remove(0);
    assert!(
        recovered.detail.contains("form is unsupported")
            && recovered.detail.contains("repair its local configuration")
    );
    assert!(
        !recovered.detail.contains("Generic old unknown outcome")
            && !recovered.detail.contains("PRIVATE_RAW_STACK")
    );
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
}

#[test]
fn cancelling_invalidates_all_interactions_and_late_native_requests_cannot_reappear() {
    for mode in ["cancel-approval", "cancel-question"] {
        let server = Server::memory(vec![
            stream(tool("chat_completions", mode)),
            stream(success("Waiting for your decision.")),
            stream(success(
                "Codex confirmed interruption; earlier effects remain.",
            )),
        ]);
        let (root, core, persona) = isolated();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), mode);
        assert_fake(&core, workspace.path());
        let (_handle, events) = start(&core, &persona, "Ask before continuing synthetic work.");
        terminal(&events);
        let task = wait(&core, |t| t[0].request.is_some()).remove(0);
        let request = task.request.unwrap();
        let answer = [TaskAnswer {
            question_id: "choice".into(),
            answers: vec!["Small".into()],
        }];
        core.cancel_delegation(&task.id).unwrap();
        let cancelling = core.delegated_tasks().unwrap().remove(0);
        assert_eq!(cancelling.state, DelegationState::CancellationRequested);
        assert!(cancelling.request.is_none());
        assert!(
            core.answer_delegation(
                &task.id,
                &request.id,
                request.approval.then_some(true),
                &answer
            )
            .is_err()
        );
        let responses = workspace.path().join("fake-responses.jsonl");
        wait(&core, |t| {
            workspace.path().join("late-request-sent").exists()
                && std::fs::read_to_string(&responses)
                    .unwrap_or_default()
                    .contains("interaction-3")
                && t[0].state == DelegationState::CancellationRequested
        });
        let late = core.delegated_tasks().unwrap().remove(0);
        assert!(late.request.is_none());
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        let saved: String = db
            .query_row("SELECT record FROM delegated_tasks", [], |r| r.get(0))
            .unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert!(saved["pending_requests"].as_array().unwrap().is_empty());
        for response in std::fs::read_to_string(&responses).unwrap().lines() {
            let value: Value = serde_json::from_str(response).unwrap();
            assert!(
                value.get("result").is_none(),
                "No answer or permission may be sent after Cancel"
            );
            assert!(value.get("error").is_some());
        }
        std::fs::write(workspace.path().join("confirm-cancel"), "synthetic").unwrap();
        let cancelled = settled(&core);
        assert_eq!(cancelled.state, DelegationState::Cancelled);
        assert!(cancelled.request.is_none());
        assert_eq!(method_count(workspace.path(), "turn/start"), 1);
        assert_eq!(method_count(workspace.path(), "turn/interrupt"), 1);
    }
}
