mod delegation_support;
mod live_support;
mod responses_support;
mod stream_bound_support;
use delegation_support::{configure as codex_configure, method_count, settled, tool, wait};
use live_support::*;
use pebble_core::{Core, DelegationState};
use serde_json::json;
use std::time::Duration;
fn final_reply(api: &str, text: &str) -> String {
    if api == "responses" {
        responses_support::success(text)
    } else {
        success(text)
    }
}

#[test]
fn both_protocols_delegate_promptly_preserve_native_replay_and_report_stored_evidence() {
    for api in ["chat_completions", "responses"] {
        let server = Server::memory(vec![
            stream(tool(api, "first")),
            stream(final_reply(
                api,
                "Task accepted; I will explain its result.",
            )),
            stream(final_reply(api, "Here is the verified synthetic evidence.")),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), "complete");
        let options = workspace.path().join("fake-options.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&options).unwrap()).unwrap();
        value["text"] = json!(
            "Verified synthetic result. https://example.org/evidence [官网](https://rust-lang.org/)显示该版本，其链接指向[官方发布公告](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/)，公告明确标注为。"
        );
        std::fs::write(options, serde_json::to_vec(&value).unwrap()).unwrap();

        let (_task, events) = start(&core, &id, "Look up the synthetic current fact.");
        let received = terminal(&events);
        assert_eq!(
            received.last().unwrap().kind,
            pebble_core::ChatEventKind::Completed,
            "{api}: {}",
            received.last().unwrap().text
        );
        let task = settled(&core);
        assert_eq!(task.state, DelegationState::Completed);
        assert_eq!(task.report_state, "completed");
        assert!(task.result.contains("Verified synthetic"));
        assert!(!task.result.contains("PRIVATE_OTHER_TASK"));
        assert!(
            task.links
                .iter()
                .any(|l| l.target == "https://example.org/evidence")
        );
        assert!(task.links.iter().any(|l| l.target.ends_with("output.txt")));
        assert!(
            task.links
                .iter()
                .any(|l| l.target == "https://rust-lang.org/")
        );
        assert!(
            task.links
                .iter()
                .any(|l| l.target == "https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/")
        );
        assert!(
            task.links
                .iter()
                .filter(|l| l.target.starts_with("http"))
                .all(|l| !l.target.contains(")") && !l.target.contains('显'))
        );

        assert_eq!(method_count(workspace.path(), "turn/start"), 1);
        let bodies = (0..3)
            .map(|_| {
                server
                    .requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
                    .1
            })
            .collect::<Vec<_>>();
        let names = bodies[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| {
                tool["name"]
                    .as_str()
                    .or(tool["function"]["name"].as_str())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "update_memory",
                "delegate_task",
                "search_history",
                "read_history"
            ]
        );
        let continuation = bodies[1].to_string();
        assert!(continuation.contains(&task.id));
        assert!(continuation.contains("delegate_task"));
        let reporting = bodies[2].to_string();
        assert!(reporting.contains("delegated_result"));
        assert!(reporting.contains("Verified synthetic result"));
        assert_eq!(bodies[2]["tool_choice"], "none");
        drop(core);
        let reopened = Core::open(root.path()).unwrap();
        assert_eq!(
            reopened.delegated_tasks().unwrap()[0].report_state,
            "completed"
        );
        assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    }
}
#[test]
fn conversation_stop_does_not_cancel_task_native_cancel_waits_for_confirmation() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "cancel")),
        stream(success("Working separately.")),
        stream(success("The task was interrupted; earlier effects remain.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "cancel");
    let (_handle, events) = start(&core, &id, "Start the synthetic long task.");
    terminal(&events);
    let task = wait(&core, |t| t.len() == 1 && t[0].turn_id.is_some()).remove(0);
    core.stop_room(&task.conversation_id).unwrap();
    assert_eq!(
        core.delegated_tasks().unwrap()[0].state,
        DelegationState::Running
    );
    core.cancel_delegation(&task.id).unwrap();
    let completed = settled(&core);
    assert_eq!(completed.state, DelegationState::Cancelled);
    assert_eq!(method_count(workspace.path(), "turn/interrupt"), 1);
}
#[test]
fn deferred_native_questions_report_limits_and_persona_followups_resume_exact_thread() {
    for mode in ["question", "approval"] {
        let server = Server::memory(vec![
            stream(tool("chat_completions", mode)),
            stream(success("I started your task.")),
            stream(success("Please clarify through chat.")),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), mode);
        let (_handle, events) = start(&core, &id, "Run the synthetic interaction.");
        terminal(&events);
        let finished = settled(&core);
        assert_eq!(finished.state, DelegationState::Interrupted);
        assert!(finished.request.is_none());
        assert!(
            core.answer_delegation(&finished.id, "\"interaction-1\"", Some(true), &[])
                .is_err()
        );
        let report = server.requests.try_iter().last().unwrap().1;
        assert_eq!(report["tool_choice"], "none");
        assert!(report.to_string().contains(if mode == "question" {
            "Which synthetic artifact?"
        } else {
            "No permission or form answer was granted"
        }));
        let continuation = Server::memory(vec![
            stream(delegation_support::tool_for(
                "chat_completions",
                "continue",
                Some(&finished.id),
            )),
            stream(success("Continuing the task.")),
            stream(success("The result is saved.")),
        ]);
        configure(
            root.path(),
            &continuation.url,
            "mock",
            "chat_completions",
            "",
        );
        std::fs::write(
            workspace.path().join("fake-options.json"),
            r#"{"mode":"complete","commentary":"Inspect the selected synthetic artifact"}"#,
        )
        .unwrap();
        let (_followup, events) = start(&core, &id, "Inspect only the small artifact.");
        terminal(&events);
        let followed = wait(&core, |t| {
            t[0].turn_id.as_deref() == Some("turn-2") && t[0].report_state == "completed"
        })
        .remove(0);
        assert_eq!(followed.state, DelegationState::Completed);
        assert_eq!(followed.title, "Inspect the selected synthetic artifact");
        assert_eq!(followed.thread_id, finished.thread_id);
        assert_eq!(method_count(workspace.path(), "thread/start"), 1);
        assert_eq!(method_count(workspace.path(), "thread/resume"), 1);
        let methods = std::fs::read_to_string(workspace.path().join("fake-methods.jsonl")).unwrap();
        assert!(methods.contains("Inspect only the small artifact."));
    }
}

#[test]
fn ambiguous_dispatch_is_never_resent_and_reconciliation_recovers_saved_result() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "loss")),
        stream(success("Accepted separately.")),
        stream(success(
            "Execution is uncertain; I cannot claim completion.",
        )),
        stream(success("Recovered from the saved execution.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "disconnect");
    let (_handle, events) = start(&core, &id, "Run the synthetic disconnected task.");
    terminal(&events);
    wait(&core, |t| {
        t.len() == 1 && t[0].state == DelegationState::Uncertain
    });
    let task = settled(&core);
    assert_eq!(task.report_state, "completed");
    assert!(core.followup_delegation(&task.id, "Do it again").is_err());
    core.reconcile_delegation(&task.id).unwrap();
    let reconciled = wait(&core, |t| {
        t[0].state == DelegationState::Completed && t[0].report_state == "completed"
    })
    .remove(0);
    assert_eq!(reconciled.state, DelegationState::Completed);
    assert!(reconciled.result.contains("Recovered actual"));
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    assert!(core.reconcile_delegation(&task.id).is_err());
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let reports: String = db
        .query_row(
            "SELECT json_extract(record,'$.reports') FROM delegated_tasks",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let reports: serde_json::Value = serde_json::from_str(&reports).unwrap();
    assert_eq!(reports.as_array().unwrap().len(), 2);
}
#[test]
fn failed_report_retry_reuses_result_without_reexecuting_task() {
    for api in ["chat_completions", "responses"] {
        let oversized = "x".repeat(128 * 1024 + 1);
        let failed = if api == "responses" {
            stream_bound_support::response_event(
                "response.reasoning_text.delta",
                json!({"output_index":0,"delta":oversized}),
            )
        } else {
            event(
                json!({"reasoning_content":oversized}),
                serde_json::Value::Null,
            )
        };
        let reply = format!("Saved evidence: {}", "x".repeat(900));
        let server = Server::memory(vec![
            stream(tool(api, "report")),
            stream(final_reply(api, "Accepted separately.")),
            stream(failed),
            stream(stream_bound_support::verbose_success(api, &reply)),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), "complete");
        let (_handle, events) = start(&core, &id, "Look up evidence.");
        terminal(&events);
        let task = settled(&core);
        assert_eq!(task.report_state, "failed");
        assert!(
            core.messages(&id)
                .unwrap()
                .last()
                .unwrap()
                .failure_detail
                .contains("decoded output limit")
        );
        core.retry_delegation_report(&task.id).unwrap();
        wait(&core, |t| t[0].report_state == "completed");
        assert_eq!(core.messages(&id).unwrap().last().unwrap().content, reply);
        assert_eq!(method_count(workspace.path(), "turn/start"), 1);
        let bodies = server
            .requests
            .try_iter()
            .map(|(_, body)| body)
            .collect::<Vec<_>>();
        assert_eq!(bodies.len(), 4);
        assert!(bodies[2..].iter().all(|body| body["tool_choice"] == "none"));
    }
}

#[test]
fn ordinary_chat_and_fresh_context_continue_while_codex_runs_and_report_uses_current_context() {
    let server = Server::new(vec![
        stream(tool("chat_completions", "background")),
        stream(success("Started in background.")),
        stream(success("Ordinary conversation still works.")),
        stream(success("Explained in my fresh context.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "gated");
    let (_first, events) = start(&core, &id, "Look up synthetic evidence in the background.");
    terminal(&events);
    wait(&core, |t| t.len() == 1 && t[0].turn_id.is_some());
    let (_second, events) = start(&core, &id, "Continue our ordinary conversation.");
    assert_eq!(
        terminal(&events).last().unwrap().kind,
        pebble_core::ChatEventKind::Completed
    );
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let before: String = db
        .query_row(
            "SELECT id FROM model_sessions WHERE status='active'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    core.fresh_context(&id).unwrap();
    let after: String = db
        .query_row(
            "SELECT id FROM model_sessions WHERE status='active'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(before, after);
    std::fs::write(workspace.path().join("release"), "").unwrap();
    let task = settled(&core);
    assert_eq!(task.report_state, "completed");
    let report: String = db
        .query_row(
            "SELECT json_extract(record,'$.report_call') FROM delegated_tasks",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let request = core.model_request(&report).unwrap();
    assert_eq!(request.session_id, after);
    let input = String::from_utf8(request.items.last().unwrap().payload.clone()).unwrap();
    assert!(input.contains("delegated_result"));
    assert!(input.contains("Verified synthetic result"));
    assert!(!input.contains("Ordinary conversation still works"));
}

#[test]
fn group_shared_task_is_deduplicated_and_explicit_mentions_can_have_distinct_assignments() {
    use std::sync::{Arc, mpsc};
    struct Listener(mpsc::Sender<pebble_core::ChatEvent>);
    impl pebble_core::ChatListener for Listener {
        fn on_event(&self, e: pebble_core::ChatEvent) -> Result<(), String> {
            self.0.send(e).map_err(|e| e.to_string())
        }
    }
    for separate in [false, true] {
        let mut plans = vec![
            stream(tool("chat_completions", "objective-A")),
            stream(success("First assignment accepted.")),
            stream(tool("chat_completions", "deliberately-different-model-key")),
            stream(success("Shared work already has an owner.")),
        ];
        plans.push(stream(success("The saved group result.")));
        if separate {
            plans.push(stream(success("The distinct group result.")));
        }
        let server = Server::memory(plans);
        let (root, core, first) = isolated();
        let second = core.create_persona("Kai").unwrap();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), "gated");
        let group = core
            .create_group("Synthetic group", &[first.clone(), second.id.clone()])
            .unwrap();
        let (tx, rx) = mpsc::channel();
        let _submission = core
            .send_room_assigned_images(
                &group.id,
                "Look up the synthetic request.",
                &[first.clone(), second.id.clone()],
                &[],
                separate,
                Arc::new(Listener(tx)),
            )
            .unwrap();
        loop {
            let e = rx.recv_timeout(Duration::from_secs(12)).unwrap();
            assert_ne!(e.kind, pebble_core::ChatEventKind::Failed, "{}", e.text);
            if e.kind == pebble_core::ChatEventKind::GroupSettled {
                break;
            }
        }
        let expected = if separate { 2 } else { 1 };
        let tasks = wait(&core, |t| {
            t.len() == expected && t.iter().all(|t| t.turn_id.is_some())
        });
        assert_eq!(tasks[0].persona_id, first);
        assert_eq!(method_count(workspace.path(), "turn/start"), expected);
        std::fs::write(workspace.path().join("release"), "").unwrap();
        wait(&core, |t| {
            t.len() == expected && t.iter().all(|t| t.report_state == "completed")
        });
        assert_eq!(method_count(workspace.path(), "turn/start"), expected);
        assert!(core.messages(&second.id).unwrap().is_empty());
    }
}
#[test]
fn removed_membership_keeps_result_card_without_publication_or_private_reroute() {
    use std::sync::{Arc, mpsc};
    struct Listener(mpsc::Sender<pebble_core::ChatEvent>);
    impl pebble_core::ChatListener for Listener {
        fn on_event(&self, e: pebble_core::ChatEvent) -> Result<(), String> {
            self.0.send(e).map_err(|e| e.to_string())
        }
    }
    let server = Server::memory(vec![
        stream(tool("chat_completions", "membership")),
        stream(success("Started your group task.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "gated");
    let group = core
        .create_group("Synthetic group", std::slice::from_ref(&id))
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let _submission = core
        .send_room_message(
            &group.id,
            "Look up synthetic evidence.",
            std::slice::from_ref(&id),
            Arc::new(Listener(tx)),
        )
        .unwrap();
    loop {
        if rx.recv_timeout(Duration::from_secs(12)).unwrap().kind
            == pebble_core::ChatEventKind::GroupSettled
        {
            break;
        }
    }
    wait(&core, |t| t.len() == 1 && t[0].turn_id.is_some());
    core.remove_group_member(&group.id, &id).unwrap();
    core.add_group_member(&group.id, &id).unwrap();
    std::fs::write(workspace.path().join("release"), "").unwrap();
    let task = settled(&core);
    assert_eq!(task.report_state, "destination_unavailable");
    assert!(core.retry_delegation_report(&task.id).is_err());
    assert_eq!(
        core.delegated_tasks().unwrap()[0].report_state,
        "destination_unavailable"
    );
    assert!(task.result.contains("Verified synthetic"));
    assert!(core.messages(&id).unwrap().is_empty());
    assert_eq!(core.room_messages(&group.id).unwrap().len(), 2);
}

#[test]
fn schema_twelve_upgrade_preserves_native_prefix_and_requires_explicit_capability_boundary() {
    let (root, core, id) = isolated();
    let (_handle, events) = start(&core, &id, "Synthetic old context.");
    terminal(&events);
    drop(_handle);
    drop(core);
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let prefix: Vec<u8> = db
        .query_row(
            "SELECT payload FROM model_items WHERE kind='prefix'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("DROP TABLE session_recent_messages; DROP TABLE automatic_rollovers; DROP TABLE summarized_events; ALTER TABLE context_maintenance DROP COLUMN automatic; ALTER TABLE model_calls DROP COLUMN history_cutoff; ALTER TABLE model_sessions DROP COLUMN history_tools; DROP TABLE image_generation_submissions; DROP TABLE image_generations; ALTER TABLE model_sessions DROP COLUMN image_generation_tools; DROP TABLE delegation_runs;DROP TABLE delegation_submissions;DROP TABLE delegated_tasks;DROP TABLE delegation_assignments;ALTER TABLE model_sessions DROP COLUMN delegation_tools;ALTER TABLE messages DROP COLUMN separate_delegations;PRAGMA user_version=12;").unwrap();
    drop(db);
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    assert!(persona.context_notice.contains("no delegation capability"));
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let after: Vec<u8> = db
        .query_row(
            "SELECT payload FROM model_items WHERE kind='prefix'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(prefix, after);
    assert!(
        !db.query_row(
            "SELECT delegation_tools FROM model_sessions WHERE status='active'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    core.fresh_context(&id).unwrap();
    assert!(
        db.query_row(
            "SELECT delegation_tools FROM model_sessions WHERE status='active'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
}
#[test]
fn restart_reconciles_saved_execution_without_a_second_dispatch() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "restart")),
        stream(success("Started separately.")),
        stream(success("Recovered the saved result.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "restart");
    let (handle, events) = start(&core, &id, "Start the restart fixture.");
    terminal(&events);
    wait(&core, |t| t.len() == 1 && t[0].turn_id.is_some());
    drop(handle);
    drop(core);
    let reopened = Core::open(root.path()).unwrap();
    let task = settled(&reopened);
    assert_eq!(task.state, DelegationState::Completed);
    assert!(task.result.contains("Recovered actual"));
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    assert_eq!(method_count(workspace.path(), "thread/read"), 1);
}

#[test]
fn managed_workspace_accepts_default_writes_or_explicit_read_only_and_bad_connections_fail() {
    for mode in [
        "managed",
        "managed-read-only",
        "missing-cli",
        "unauthenticated",
    ] {
        let server = Server::memory(vec![
            stream(tool("chat_completions", mode)),
            stream(success("Task accepted.")),
            stream(success("Here is the actual outcome.")),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), mode);
        let mut settings = core.codex_settings().unwrap();
        if mode.starts_with("managed") {
            settings.writable = if mode == "managed" {
                pebble_core::CodexSettings::default().writable
            } else {
                false
            };
        }
        if mode == "missing-cli" {
            settings.executable = workspace.path().join("absent-codex").display().to_string();
        }
        core.save_codex_settings(&settings).unwrap();
        if mode == "unauthenticated" {
            assert!(!core.codex_capabilities().unwrap().authenticated);
        }
        let (_handle, events) = start(
            &core,
            &id,
            "Verify tomorrow's synthetic fact; no personal data.",
        );
        terminal(&events);
        let task = settled(&core);
        if mode.starts_with("managed") {
            assert_eq!(task.state, DelegationState::Completed);
            let cwd = root.path().join("data/codex-workspaces").join(&task.id);
            assert_eq!(method_count(workspace.path(), "turn/start"), 1);
            assert!(task.links.iter().any(|l| l.target.ends_with("output.txt")));
            let records =
                std::fs::read_to_string(workspace.path().join("fake-methods.jsonl")).unwrap();
            let connection = records
                .lines()
                .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
                .find(|v| v["method"] == "thread/start")
                .unwrap();
            assert_eq!(
                connection["params"]["cwd"],
                cwd.canonicalize().unwrap().display().to_string()
            );
            assert_eq!(connection["params"]["approvalPolicy"], "on-request");
            assert_eq!(connection["params"]["approvalsReviewer"], "auto_review");
            assert_eq!(
                connection["params"]["sandbox"],
                if mode == "managed" {
                    "workspace-write"
                } else {
                    "read-only"
                }
            );
            let start = records
                .lines()
                .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
                .find(|v| v["method"] == "turn/start")
                .unwrap();
            let input: serde_json::Value =
                serde_json::from_str(start["params"]["input"][0]["text"].as_str().unwrap())
                    .unwrap();
            assert_eq!(
                input["original_user_request"],
                "Verify tomorrow's synthetic fact; no personal data."
            );
            assert!(
                input["runtime_time"]["local_date_time_and_offset"]
                    .as_str()
                    .is_some()
            );
            assert_eq!(input["workspace"], cwd.display().to_string());
            assert_eq!(input["workspace_writable"], mode == "managed");
            assert_eq!(
                std::fs::read_to_string(cwd.join("output.txt")).unwrap(),
                "synthetic artifact"
            );
        } else {
            assert_eq!(task.state, DelegationState::Failed);
            assert_eq!(method_count(workspace.path(), "turn/start"), 0);
            let detail = task.detail.to_ascii_lowercase();
            assert!(
                detail.contains("executable") || detail.contains("login"),
                "{}",
                task.detail
            );
            if mode == "unauthenticated" {
                assert!(task.detail.contains("Login required"));
            }
        }
    }
}

#[test]
fn ordinary_retry_of_a_failed_report_after_fresh_context_explains_saved_result_only() {
    struct Ignore;
    impl pebble_core::ChatListener for Ignore {
        fn on_event(&self, _: pebble_core::ChatEvent) -> Result<(), String> {
            Ok(())
        }
    }
    let server = Server::new(vec![
        stream(tool("chat_completions", "report-retry")),
        stream(success("Started.")),
        stream(event(json!({}), json!("length"))),
        stream(event(json!({}), json!("length"))),
        stream(stream_bound_support::verbose_success(
            "chat_completions",
            &format!("Explained saved evidence. {}", "x".repeat(900)),
        )),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "complete");
    let (_handle, events) = start(&core, &id, "Look up synthetic evidence.");
    terminal(&events);
    let task = settled(&core);
    assert_eq!(task.report_state, "failed");
    core.fresh_context(&id).unwrap();
    for attempt in 0..2 {
        let last = core.messages(&id).unwrap().pop().unwrap();
        assert!(
            last.can_retry,
            "attempt={attempt}, last={last:?}, task={:?}",
            core.delegated_tasks().unwrap()
        );
        let _retry = core
            .retry_message(&id, &last.id, std::sync::Arc::new(Ignore))
            .unwrap();
        wait(&core, |t| {
            t[0].report_state == if attempt == 0 { "failed" } else { "completed" }
        });
    }
    assert!(core.retry_delegation_report(&task.id).is_err());
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let record: String = db
        .query_row("SELECT record FROM delegated_tasks", [], |r| r.get(0))
        .unwrap();
    let record: serde_json::Value = serde_json::from_str(&record).unwrap();
    let reports = record["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 3);
    let active: String = db
        .query_row(
            "SELECT id FROM model_sessions WHERE status='active'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for report in &reports[1..] {
        let request = core
            .model_request(report["call"].as_str().unwrap())
            .unwrap();
        assert_eq!(request.session_id, active);
        assert!(!request.memory_tools && !request.delegation_tools);
        assert!(
            String::from_utf8(request.items.last().unwrap().payload.clone())
                .unwrap()
                .contains("Verified synthetic result")
        );
    }
}

#[test]
fn current_group_message_claim_covers_prior_task_followups_and_new_work() {
    use std::sync::{Arc, mpsc};
    struct Listener(mpsc::Sender<pebble_core::ChatEvent>);
    impl pebble_core::ChatListener for Listener {
        fn on_event(&self, e: pebble_core::ChatEvent) -> Result<(), String> {
            self.0.send(e).map_err(|e| e.to_string())
        }
    }
    for (separate, second_new) in [(false, false), (false, true), (true, false), (true, true)] {
        let initial = Server::memory(vec![
            stream(tool("chat_completions", "prior-a")),
            stream(success("A started.")),
            stream(tool("chat_completions", "prior-b")),
            stream(success("B started.")),
            stream(success("A evidence.")),
            stream(success("B evidence.")),
        ]);
        let (root, core, first) = isolated();
        let second = core.create_persona("Kai").unwrap();
        configure(root.path(), &initial.url, "mock", "chat_completions", "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), "gated");
        let group = core
            .create_group("Followup claims", &[first.clone(), second.id.clone()])
            .unwrap();
        let mentions = [first.clone(), second.id.clone()];
        let send = |text: &str, separate: bool| {
            delegation_support::assert_fake(&core, workspace.path());
            let (tx, rx) = mpsc::channel();
            let _submission = core
                .send_room_assigned_images(
                    &group.id,
                    text,
                    &mentions,
                    &[],
                    separate,
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
        };
        send("Two deliberately separate original tasks.", true);
        wait(&core, |t| {
            t.len() == 2 && t.iter().all(|t| t.turn_id.is_some())
        });
        std::fs::write(workspace.path().join("release"), "").unwrap();
        let prior = wait(&core, |t| {
            t.len() == 2 && t.iter().all(|t| t.report_state == "completed")
        });
        std::fs::remove_file(workspace.path().join("release")).unwrap();
        let a = prior.iter().find(|t| t.persona_id == first).unwrap();
        let b = prior.iter().find(|t| t.persona_id == second.id).unwrap();
        let mut plans = vec![
            stream(delegation_support::tool_for(
                "chat_completions",
                "follow-a",
                Some(&a.id),
            )),
            stream(success("A continued.")),
            stream(delegation_support::tool_for(
                "chat_completions",
                "follow-b",
                if second_new { None } else { Some(&b.id) },
            )),
            stream(success("B claim checked.")),
            stream(success("New outcome.")),
        ];
        if separate {
            plans.push(stream(success("Separate outcome.")));
        }
        let follow = Server::memory(plans);
        configure(root.path(), &follow.url, "mock", "chat_completions", "");
        codex_configure(&core, workspace.path(), "gated");
        send("Continue work for this current request.", separate);
        let expected = 3 + usize::from(separate);
        wait(&core, |t| {
            t.iter().filter(|t| t.state.active()).count() == if separate { 2 } else { 1 }
                && method_count(workspace.path(), "turn/start") == expected
        });
        std::fs::write(workspace.path().join("release"), "").unwrap();
        let tasks = wait(&core, |t| t.iter().all(|t| t.report_state == "completed"));
        assert_eq!(method_count(workspace.path(), "turn/start"), expected);
        assert_eq!(tasks.len(), if separate && second_new { 3 } else { 2 });
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        let claims:Vec<String>=db.prepare("SELECT s.task_id FROM delegation_submissions s JOIN model_calls c ON c.id=s.owner_id JOIN messages m ON m.id=c.trigger_message_id WHERE m.content LIKE '%Continue work for this current request.' ORDER BY s.rowid").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[0], a.id);
        assert_eq!(claims[0] == claims[1], !separate);
        let original: String = db
            .query_row(
                "SELECT trigger_id FROM delegated_tasks WHERE id=?1",
                [&a.id],
                |r| r.get(0),
            )
            .unwrap();
        let original_text: String = db
            .query_row(
                "SELECT content FROM messages WHERE id=?1",
                [original],
                |r| r.get(0),
            )
            .unwrap();
        assert!(original_text.ends_with("Two deliberately separate original tasks."));
    }
}
