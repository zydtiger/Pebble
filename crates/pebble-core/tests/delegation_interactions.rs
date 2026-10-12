mod delegation_support;
mod live_support;
mod responses_support;
use delegation_support::{
    assert_fake, configure as codex_configure, method_count, settled, tool, wait,
};
use live_support::*;
use pebble_core::DelegationState;
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
    let task = settled(&core);
    assert!(task.request.is_none());
    assert_eq!(task.state, DelegationState::Interrupted);
    assert!(task.detail.contains("form is unsupported"));
    assert_eq!(method_count(workspace.path(), "turn/interrupt"), 1);
    let response: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
    )
    .unwrap();
    assert!(response.get("error").is_some());
    assert!(response.get("result").is_none());
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
        let task = settled(&core);
        assert_eq!(task.state, DelegationState::Interrupted);
        assert!(task.detail.contains("full scope exceeds"));
        assert!(
            task.request.is_none(),
            "An oversized approval must never expose Allow once"
        );
        assert!(
            core.answer_delegation(&task.id, "\"interaction-1\"", Some(true), &[])
                .is_err()
        );
        assert_eq!(method_count(workspace.path(), "turn/interrupt"), 1);
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
    let task = settled(&core);
    assert!(task.request.is_none() && task.links.is_empty());
    assert_eq!(task.state, DelegationState::Interrupted);
    assert!(task.detail.contains("unsupported authorization URL"));
    assert_eq!(method_count(workspace.path(), "turn/interrupt"), 1);
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
