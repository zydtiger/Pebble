mod delegation_support;
mod live_support;
use delegation_support::{
    assert_fake, configure as codex_configure, method_count, settled, tool, wait,
};
use live_support::*;
use pebble_core::{Core, DelegatedTask, DelegationState};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

fn saved_record(root: &Path, task: &DelegatedTask) -> Value {
    let text: String = rusqlite::Connection::open(root.join("data/app.sqlite"))
        .unwrap()
        .query_row(
            "SELECT record FROM delegated_tasks WHERE id=?1",
            [&task.id],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn lost_turn_start_response_recovers_by_user_client_id_and_reports_once() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "lost-response")),
        stream(success("Started separately.")),
        stream(success("The execution outcome is uncertain.")),
        stream(success("Recovered the accepted turn's evidence.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "turn-start-loss");
    assert_fake(&core, workspace.path());
    let (handle, events) = start(&core, &persona, "Run the synthetic response-loss task.");
    terminal(&events);
    let task = settled(&core);
    assert_eq!(task.state, DelegationState::Uncertain);
    assert!(task.thread_id.is_some());
    assert!(task.turn_id.is_none());
    let before = saved_record(root.path(), &task);
    assert_eq!(before["dispatch_started"], true);
    assert_eq!(before["reports"].as_array().unwrap().len(), 1);
    let saved: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join(format!(
            "fake-saved-{}.json",
            task.thread_id.as_ref().unwrap()
        )))
        .unwrap(),
    )
    .unwrap();
    assert_ne!(saved["items"][0]["id"], before["submission"]);
    assert_eq!(saved["items"][0]["clientId"], before["submission"]);
    core.reconcile_delegation(&task.id).unwrap();
    let recovered = wait(&core, |tasks| {
        tasks[0].state == DelegationState::Completed && tasks[0].report_state == "completed"
    })
    .remove(0);
    assert_eq!(recovered.turn_id.as_deref(), Some("turn-1"));
    assert!(recovered.result.contains("Recovered accepted turn"));
    assert_eq!(recovered.links.len(), 1);
    assert_eq!(
        recovered.links[0].target,
        "https://example.org/accepted-turn"
    );
    let after = saved_record(root.path(), &recovered);
    assert_eq!(after["reports"].as_array().unwrap().len(), 2);
    assert_eq!(after["reports"][1]["input"]["status"], "completed");
    assert_eq!(after["reports"][1]["input"]["result"], recovered.result);
    assert!(core.reconcile_delegation(&task.id).is_err());
    drop(handle);
    drop(core);
    let reopened = Core::open(root.path()).unwrap();
    let persisted = settled(&reopened);
    assert_eq!(persisted.state, DelegationState::Completed);
    assert_eq!(persisted.result, recovered.result);
    assert_eq!(persisted.links, recovered.links);
    assert_eq!(
        saved_record(root.path(), &persisted)["reports"],
        after["reports"]
    );
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    assert_eq!(method_count(workspace.path(), "thread/read"), 1);
    for _ in 0..4 {
        server
            .requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
    }
    assert!(
        server
            .requests
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
}

#[test]
fn unrelated_user_client_id_stays_uncertain_across_reconciliation_and_reload() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "unrelated-client")),
        stream(success("Started separately.")),
        stream(success("The execution outcome is uncertain.")),
        stream(success(
            "The saved thread does not confirm this submission.",
        )),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "turn-start-loss-mismatch");
    assert_fake(&core, workspace.path());
    let (handle, events) = start(&core, &persona, "Run only this synthetic request.");
    terminal(&events);
    let task = settled(&core);
    assert_eq!(task.state, DelegationState::Uncertain);
    assert!(task.thread_id.is_some());
    assert!(task.turn_id.is_none());
    core.reconcile_delegation(&task.id).unwrap();
    let unconfirmed = wait(&core, |tasks| {
        tasks[0].detail.contains("does not confirm this submission")
            && tasks[0].report_state == "completed"
    })
    .remove(0);
    assert_eq!(unconfirmed.state, DelegationState::Uncertain);
    assert!(unconfirmed.turn_id.is_none());
    assert!(unconfirmed.result.is_empty());
    assert!(unconfirmed.links.is_empty());
    let before = saved_record(root.path(), &unconfirmed);
    assert!(core.followup_delegation(&task.id, "Continue").is_err());
    drop(handle);
    drop(core);
    let reopened = Core::open(root.path()).unwrap();
    let persisted = wait(&reopened, |tasks| {
        method_count(workspace.path(), "thread/read") == 2 && tasks[0].report_state == "completed"
    })
    .remove(0);
    assert_eq!(persisted.state, DelegationState::Uncertain);
    assert!(persisted.turn_id.is_none());
    assert!(persisted.result.is_empty());
    assert!(persisted.links.is_empty());
    assert_eq!(
        saved_record(root.path(), &persisted)["reports"],
        before["reports"]
    );
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    for _ in 0..4 {
        server
            .requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
    }
    assert!(
        server
            .requests
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
}

#[test]
fn known_authorization_echo_is_redacted_after_reopen_reconcile_and_followup_history() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "auth-recovery")),
        stream(success("Started separately.")),
        stream(success("The outcome is not confirmed.")),
        stream(success("Recovered the actual source.")),
        stream(success("The continuation's actual source is saved.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "url-loss");
    assert_fake(&core, workspace.path());
    let auth = "https://example.org/authorize?state=secret";
    let text = format!(
        "Authorization was completed at [connected tool]({auth}). Genuine source: https://example.org/actual-source"
    );
    std::fs::write(workspace.path().join("fake-options.json"), serde_json::json!({"mode":"url-loss","text":text,"status":"failed","runtime_error":format!("Synthetic tool failed after authorization: {auth} . Check local tool configuration.")}).to_string()).unwrap();
    let (handle, events) = start(&core, &persona, "Use only the synthetic connected tool.");
    terminal(&events);
    let uncertain = settled(&core);
    assert_eq!(uncertain.state, DelegationState::Uncertain);
    let digest = saved_record(root.path(), &uncertain)["authorization_url_digests"][0]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(digest.len(), 64);
    assert!(!serde_json::to_string(&uncertain).unwrap().contains(auth));
    drop(handle);
    drop(core);
    let reopened = Core::open(root.path()).unwrap();
    let recovered = wait(&reopened, |t| {
        t[0].state == DelegationState::Interrupted && t[0].report_state == "completed"
    })
    .remove(0);
    assert!(
        recovered
            .detail
            .contains("Synthetic tool failed after authorization: [authorization link removed]")
    );
    assert_eq!(recovered.thread_id, uncertain.thread_id);
    assert!(recovered.result.contains("[authorization link removed]"));
    assert_eq!(
        recovered
            .links
            .iter()
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        vec!["https://example.org/actual-source"]
    );
    assert!(
        !saved_record(root.path(), &recovered)
            .to_string()
            .contains("state=secret")
    );
    let next_auth = format!("{auth}&code=secret2");
    std::fs::write(workspace.path().join("fake-options.json"), serde_json::json!({"mode":"url", "url":next_auth, "text":format!("Earlier tool URL: {auth} . Current tool URL: {next_auth} . Current source: https://example.org/current-source")}).to_string()).unwrap();
    reopened
        .followup_delegation(
            &recovered.id,
            "Continue reading synthetic evidence in the same scope.",
        )
        .unwrap();
    let continued = wait(&reopened, |t| {
        t[0].state == DelegationState::Interrupted
            && t[0].turn_id.as_deref() == Some("turn-2")
            && t[0].report_state == "completed"
    })
    .remove(0);
    assert_eq!(continued.thread_id, recovered.thread_id);
    assert!(continued.result.contains("[authorization link removed]"));
    assert!(!continued.result.contains("secret2"));
    assert_eq!(
        continued
            .links
            .iter()
            .filter(|l| l.target.starts_with("https://"))
            .map(|l| l.target.as_str())
            .collect::<Vec<_>>(),
        vec!["https://example.org/current-source"]
    );
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let historical: String = db
        .query_row(
            "SELECT record FROM delegation_runs WHERE task_id=?1",
            [&continued.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(historical.contains("actual-source"));
    assert!(!historical.contains("state=secret"));
    for _ in 0..5 {
        let body = server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .1
            .to_string();
        assert!(
            !body.contains("state=secret")
                && !body.contains("/authorize")
                && !body.contains(&digest)
        );
    }
    drop(db);
    drop(reopened);
    let stable = Core::open(root.path()).unwrap();
    let task = settled(&stable);
    assert_eq!(task.result, continued.result);
    assert_eq!(task.links, continued.links);
    assert_eq!(method_count(workspace.path(), "turn/start"), 2);
    assert_eq!(method_count(workspace.path(), "thread/read"), 1);
}

#[test]
fn standard_turn_start_rejections_fail_but_internal_and_unknown_errors_remain_uncertain() {
    for code in [-32600, -32601, -32602, -32603, -32000] {
        let server = Server::memory(vec![
            stream(tool("chat_completions", "rejection")),
            stream(success("Started separately.")),
            stream(success("The actual startup error is saved.")),
        ]);
        let (root, core, persona) = isolated();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), "turn-rejected");
        assert_fake(&core, workspace.path());
        std::fs::write(
            workspace.path().join("fake-options.json"),
            serde_json::json!({"mode":"turn-rejected","code":code}).to_string(),
        )
        .unwrap();
        let (handle, events) = start(
            &core,
            &persona,
            "Run a synthetic request with a rejected effort.",
        );
        terminal(&events);
        let task = settled(&core);
        let known = [-32600, -32601, -32602].contains(&code);
        assert_eq!(
            task.state,
            if known {
                DelegationState::Failed
            } else {
                DelegationState::Uncertain
            }
        );
        assert!(task.thread_id.is_some() && task.turn_id.is_none());
        assert!(task.detail.contains("Synthetic invalid effort"));
        assert_eq!(method_count(workspace.path(), "turn/start"), 1);
        assert_eq!(method_count(workspace.path(), "thread/read"), 0);
        let record = saved_record(root.path(), &task);
        assert_eq!(record["reports"][0]["input"]["detail"], task.detail);
        for _ in 0..3 {
            server
                .requests
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
        }
        drop(handle);
        if known {
            assert!(core.reconcile_delegation(&task.id).is_err());
            drop(core);
            let reopened = Core::open(root.path()).unwrap();
            assert_eq!(settled(&reopened).state, DelegationState::Failed);
            assert_eq!(method_count(workspace.path(), "turn/start"), 1);
            assert_eq!(method_count(workspace.path(), "thread/read"), 0);
        }
    }
}

#[test]
fn persona_reconciles_uncertain_saved_outcome_without_new_turn_or_changed_submission() {
    let initial = Server::memory(vec![
        stream(tool("chat_completions", "uncertain")),
        stream(success("Started.")),
        stream(success("The outcome is uncertain.")),
    ]);
    let (root, core, owner) = isolated();
    configure(root.path(), &initial.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "disconnect");
    let (_handle, events) = start(&core, &owner, "Inspect the synthetic workspace.");
    terminal(&events);
    let before = settled(&core);
    assert_eq!(before.state, DelegationState::Uncertain);
    let before_record = saved_record(root.path(), &before);
    let follow = Server::memory(vec![
        stream(delegation_support::tool_for(
            "chat_completions",
            "reconcile",
            Some(&before.id),
        )),
        stream(success("I am checking only the saved outcome.")),
        stream(success("Here is the confirmed saved result.")),
    ]);
    configure(root.path(), &follow.url, "mock", "chat_completions", "");
    let (_handle, events) = start(
        &core,
        &owner,
        "What happened to that task? Check its saved result.",
    );
    terminal(&events);
    let after = wait(&core, |t| {
        t[0].state == DelegationState::Completed && t[0].report_state == "completed"
    })
    .remove(0);
    let after_record = saved_record(root.path(), &after);
    assert_eq!(after_record["generation"], before_record["generation"]);
    assert_eq!(after_record["submission"], before_record["submission"]);
    assert_eq!(after.thread_id, before.thread_id);
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    assert_eq!(method_count(workspace.path(), "thread/read"), 1);
    assert!(after.result.contains("Recovered actual"));
    let requests = follow
        .requests
        .try_iter()
        .map(|(_, r)| r)
        .collect::<Vec<_>>();
    assert!(
        requests[1]
            .to_string()
            .contains("No task execution was resent")
    );
    assert_eq!(requests.last().unwrap()["tool_choice"], "none");
}

#[test]
fn saved_outcome_reconciliation_rejects_other_owner_and_other_room() {
    use std::sync::{Arc, mpsc};
    struct Listener(mpsc::Sender<pebble_core::ChatEvent>);
    impl pebble_core::ChatListener for Listener {
        fn on_event(&self, event: pebble_core::ChatEvent) -> Result<(), String> {
            self.0.send(event).map_err(|e| e.to_string())
        }
    }
    let initial = Server::memory(vec![
        stream(tool("chat_completions", "uncertain")),
        stream(success("Started.")),
        stream(success("The outcome is uncertain.")),
    ]);
    let (root, core, owner) = isolated();
    configure(root.path(), &initial.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "disconnect");
    let (_handle, events) = start(&core, &owner, "Inspect only synthetic data.");
    terminal(&events);
    let task = settled(&core);
    let other = core.create_persona("Kai").unwrap();
    for cross_room in [false, true] {
        let follow = Server::memory(vec![
            stream(delegation_support::tool_for(
                "chat_completions",
                "forbidden",
                Some(&task.id),
            )),
            stream(success(
                "This task belongs to its original character and room.",
            )),
        ]);
        configure(root.path(), &follow.url, "mock", "chat_completions", "");
        if cross_room {
            let group = core
                .create_group("Other room", std::slice::from_ref(&owner))
                .unwrap();
            let (tx, rx) = mpsc::channel();
            let _submission = core
                .send_room_assigned_images(
                    &group.id,
                    "Check the earlier task.",
                    std::slice::from_ref(&owner),
                    &[],
                    false,
                    Arc::new(Listener(tx)),
                )
                .unwrap();
            loop {
                let event = rx.recv_timeout(Duration::from_secs(12)).unwrap();
                assert_ne!(
                    event.kind,
                    pebble_core::ChatEventKind::Failed,
                    "{}",
                    event.text
                );
                if event.kind == pebble_core::ChatEventKind::GroupSettled {
                    break;
                }
            }
        } else {
            let (_handle, events) = start(&core, &other.id, "Check the other character's task.");
            terminal(&events);
        }
        let request = follow.requests.try_iter().last().unwrap().1;
        assert!(
            request
                .to_string()
                .contains("Only the task's reporting owner in its original room")
        );
    }
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    assert_eq!(method_count(workspace.path(), "thread/read"), 0);
    assert_eq!(
        core.delegated_tasks().unwrap()[0].state,
        DelegationState::Uncertain
    );
}
