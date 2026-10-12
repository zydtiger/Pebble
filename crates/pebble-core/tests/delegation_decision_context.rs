mod delegation_support;
mod live_support;
mod responses_support;
use delegation_support::{assert_fake, configure as codex_configure, method_count, settled, tool};
use live_support::*;
use pebble_core::DelegationState;
use serde_json::{Value, json};

#[test]
fn native_interactions_are_deferred_without_answers_or_permission_and_report_through_persona() {
    for mode in [
        "question",
        "secret-question",
        "approval",
        "approval-network",
        "approval-stdin",
        "approval-actions",
        "file-overlap",
        "overlap",
        "form-mixed",
        "form-optional",
        "form-choices",
        "form-constrained",
        "url",
    ] {
        let server = Server::memory(vec![
            stream(tool("chat_completions", mode)),
            stream(success("Started.")),
            stream(success(
                "I need clarification or configuration before continuing.",
            )),
        ]);
        let (root, core, persona) = isolated();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), mode);
        if mode == "secret-question" {
            std::fs::write(
                workspace.path().join("fake-options.json"),
                json!({"mode":"question","secret_question":true}).to_string(),
            )
            .unwrap();
        }
        assert_fake(&core, workspace.path());
        let (_handle, events) = start(
            &core,
            &persona,
            "Inspect only synthetic evidence; ask before changing scope.",
        );
        terminal(&events);
        let task = settled(&core);
        assert_eq!(
            task.state,
            DelegationState::Interrupted,
            "{mode}: {}",
            task.detail
        );
        assert!(task.request.is_none());
        assert!(
            task.detail.contains("no input remains pending")
                || task.detail.contains("no approval remains pending")
        );
        if ["question", "overlap"].contains(&mode) {
            for text in [
                "Which synthetic artifact?",
                "Scope and cost",
                "Read one synthetic file; no modifications.",
                "higher latency",
            ] {
                assert!(task.detail.contains(text), "{text}");
            }
        } else {
            assert!(
                task.detail
                    .contains("No permission or form answer was granted")
            );
            assert!(!task.detail.contains("SECRET_DEFAULT"));
            if mode == "secret-question" {
                assert!(!task.detail.contains("Which synthetic artifact?"));
            }
        }
        let record: String = rusqlite::Connection::open(root.path().join("data/app.sqlite"))
            .unwrap()
            .query_row("SELECT record FROM delegated_tasks", [], |r| r.get(0))
            .unwrap();
        let saved: Value = serde_json::from_str(&record).unwrap();
        assert!(saved["pending_requests"].as_array().unwrap().is_empty());
        let reports = server
            .requests
            .try_iter()
            .map(|(_, request)| request)
            .collect::<Vec<_>>();
        let report = reports.last().unwrap();
        assert_eq!(report["tool_choice"], "none");
        assert!(
            report.to_string().contains("no input remains pending")
                || report.to_string().contains("no approval remains pending")
        );
        assert!(!report.to_string().contains("state=secret"));
        let responses =
            std::fs::read_to_string(workspace.path().join("fake-responses.jsonl")).unwrap();
        let responses = responses
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(!responses.is_empty());
        for response in &responses {
            assert!(
                response.get("error").is_some()
                    || response["result"]["decision"] == "decline"
                    || response["result"]["action"] == "decline"
                    || response["result"]["permissions"] == json!({})
            );
            assert!(response["result"].get("answers").is_none());
            assert!(response["result"].get("content").is_none());
        }
        if mode == "overlap" {
            assert_eq!(
                responses
                    .iter()
                    .filter(|r| r["id"] == "interaction-2")
                    .count(),
                1
            );
        }
        assert_eq!(method_count(workspace.path(), "turn/start"), 1);
        assert_eq!(method_count(workspace.path(), "turn/interrupt"), 1);
    }
}

#[test]
fn deferred_question_without_interrupt_confirmation_stays_uncertain_and_cannot_be_resent() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "uncertain-question")),
        stream(success("Started.")),
        stream(success("The interruption is not confirmed.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "question");
    std::fs::write(
        workspace.path().join("fake-options.json"),
        json!({"mode":"question","interrupt_unconfirmed":true}).to_string(),
    )
    .unwrap();
    let (_handle, events) = start(&core, &persona, "Inspect synthetic data.");
    terminal(&events);
    let task = settled(&core);
    assert_eq!(task.state, DelegationState::Uncertain);
    assert!(task.detail.contains("Which synthetic artifact?"));
    assert!(core.followup_delegation(&task.id, "Small").is_err());
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
}

#[test]
fn arbitrary_pattern_constraint_is_explicitly_unsupported_without_an_answer() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "pattern")),
        stream(success("Started.")),
        stream(success("Unsupported constraint.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "form-pattern");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &persona, "Review a synthetic arbitrary form.");
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
    assert!(response.get("error").is_some() && response.get("result").is_none());
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("fake-responses.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1,
        "Stale and duplicate native requests must not receive another response"
    );
}
