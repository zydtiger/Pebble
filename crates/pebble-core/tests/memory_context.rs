mod live_support;
mod responses_support;
use live_support::*;
use pebble_core::{ChatEventKind, Core, MaintenanceTask};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
fn tool(api: &str, content: &str) -> String {
    tool_args(
        api,
        "update_memory",
        &json!({"content":content}).to_string(),
    )
}
fn tool_args(api: &str, name: &str, args: &str) -> String {
    if api == "responses" {
        format!(
            "data: {}\n\n",
            json!({"type":"response.completed","response":{"status":"completed","output":[{"id":"reasoning-memory","type":"reasoning","encrypted_content":"opaque-memory"},{"id":"native-memory","type":"function_call","status":"completed","call_id":"memory-1","name":name,"arguments":args}]}})
        )
    } else {
        format!(
            "{}{}{}data: [DONE]\n\n",
            event(
                json!({"reasoning_content":"private tool reasoning","tool_calls":[{"index":0,"id":"memory-1","type":"function","function":{"name":name,"arguments":args}}]}),
                Value::Null
            ),
            event(json!({"content":null}), Value::Null),
            event(json!({}), json!("tool_calls"))
        )
    }
}
fn wait(task: &Arc<MaintenanceTask>) -> pebble_core::MaintenanceStatus {
    for _ in 0..500 {
        let s = task.status();
        if s.status != "running" {
            return s;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("maintenance timed out")
}
fn db(root: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap()
}
fn memory(core: &Core, id: &str) -> String {
    core.workspace()
        .unwrap()
        .personas
        .into_iter()
        .find(|p| p.id == id)
        .unwrap()
        .memory
}
fn state(root: &Path, id: &str) -> String {
    db(root)
        .query_row(
            "SELECT current_state FROM persona_states WHERE persona_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}
#[test]
fn tool_effect_survives_failed_final_restart_and_retry_without_reexecution() {
    for api in ["chat_completions", "responses"] {
        let final_body = if api == "responses" {
            format!("data: {}\n\n", json!({"type":"response.failed"}))
        } else {
            event(json!({}), json!("length"))
        };
        let accepted = if api == "responses" {
            responses_success("retried final")
        } else {
            success("retried final")
        };
        let server = Server::memory(vec![
            stream(tool(api, "# Memory\n\nSynthetic preference: blue.")),
            stream(final_body),
            stream(accepted),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let mut settings = core.settings(None).unwrap();
        settings.chat.request_json =
            r#"{"temperature":0,"chat_template_kwargs":{"enable_thinking":false}}"#.into();
        core.save_settings(
            None,
            &pebble_core::SettingsUpdate {
                chat: settings.chat,
                credential: pebble_core::CredentialChange::Keep,
                persona: None,
            },
        )
        .unwrap();
        let (task, rx) = start(&core, &id, "Remember my synthetic preference.");
        let terminal_event = terminal(&rx).pop().unwrap();
        assert_eq!(terminal_event.kind, ChatEventKind::Failed);
        assert!(terminal_event.text.contains("Memory was saved"));
        assert_eq!(
            memory(&core, &id),
            "# Memory\n\nSynthetic preference: blue."
        );
        assert_eq!(
            serde_json::from_str::<Value>(&state(root.path(), &id)).unwrap()["affinity_user"],
            20
        );
        let failed = core.messages(&id).unwrap()[1].clone();
        assert!(failed.content.is_empty());
        let initial = server.requests.recv().unwrap().1;
        assert_eq!(initial["temperature"], 0);
        assert_eq!(initial["chat_template_kwargs"]["enable_thinking"], false);
        assert!(initial.get("response_format").is_none());
        assert!(initial.get("text").is_none());
        let continuation = server.requests.recv().unwrap().1;
        assert_eq!(continuation["temperature"], 0);
        assert_eq!(
            continuation["chat_template_kwargs"]["enable_thinking"],
            false
        );
        assert_eq!(continuation["tool_choice"], "none");
        let field = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        assert!(continuation[field].to_string().contains("memory-1"));
        let before = core.model_request(&task.id).unwrap();
        assert!(
            before
                .items
                .iter()
                .any(|i| String::from_utf8_lossy(&i.payload).contains("Memory saved"))
        );
        drop(task);
        drop(core);
        let core = reopen(root.path());
        let mut settings = core.settings(None).unwrap();
        settings.chat.request_json =
            r#"{"temperature":0.25,"chat_template_kwargs":{"enable_thinking":false}}"#.into();
        core.save_settings(
            None,
            &pebble_core::SettingsUpdate {
                chat: settings.chat,
                credential: pebble_core::CredentialChange::Keep,
                persona: None,
            },
        )
        .unwrap();
        let (retried, rx) = retry(&core, &id, &failed.id);
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let replay = server.requests.recv().unwrap().1;
        assert_eq!(replay["temperature"], 0.25);
        assert_eq!(replay["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(replay["tool_choice"], "none");
        assert_eq!(
            db(root.path())
                .query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            core.messages(&id).unwrap().last().unwrap().content,
            "retried final"
        );
        drop(retried);
    }
}
fn responses_success(text: &str) -> String {
    let content = structured(text);
    format!(
        "data: {}\n\ndata: {}\n\n",
        json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":content}),
        json!({"type":"response.completed","response":{"status":"completed","output":[{"id":"final","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":content}]}]}})
    )
}
#[test]
fn manual_memory_snapshot_is_visible_and_later_conflict_is_preserved() {
    let server = Server::memory(vec![Plan {
        status: 200,
        body: tool("chat_completions", "unseen replacement"),
        delay_headers: true,
        delay_tail: false,
    }]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let persona = core.workspace().unwrap().personas.remove(0);
    let path = Path::new(&persona.directory).join("MEMORY.md");
    std::fs::write(&path, "Manual addition before acceptance.").unwrap();
    let (_task, rx) = start(&core, &id, "Remember something");
    let request = server.requests.recv().unwrap().1;
    assert!(
        request["messages"]
            .to_string()
            .contains("Manual addition before acceptance")
    );
    std::fs::write(&path, "Manual edit during HTTP.").unwrap();
    let event = terminal(&rx).pop().unwrap();
    assert_eq!(event.kind, ChatEventKind::Failed);
    assert!(event.text.contains("changed"));
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "Manual edit during HTTP."
    );
}
#[test]
fn malformed_unknown_and_oversized_arguments_fail_without_effects_or_repairs() {
    for (name, args) in [
        ("unknown", json!({"content":"x"}).to_string()),
        ("update_memory", "{broken".into()),
        (
            "update_memory",
            json!({"content":"x","path":"other"}).to_string(),
        ),
        (
            "update_memory",
            json!({"content":"x".repeat(8193)}).to_string(),
        ),
    ] {
        let server = Server::memory(vec![stream(tool_args("chat_completions", name, &args))]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "chat_completions", "");
        let old = memory(&core, &id);
        let (_task, rx) = start(&core, &id, "test");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Failed);
        assert_eq!(memory(&core, &id), old);
        assert_eq!(
            db(root.path())
                .query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
#[test]
fn maintenance_consolidates_silent_group_observations_and_preserves_post_cutoff_events() {
    for api in ["chat_completions", "responses"] {
        let server = Server::memory(vec![
            Plan {
                status: 200,
                body: tool(api, "Silent group knowledge saved."),
                delay_headers: true,
                delay_tail: false,
            },
            stream(if api == "responses" {
                responses_success("after boundary")
            } else {
                success("after boundary")
            }),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let mut settings = core.settings(None).unwrap();
        settings.chat.request_json =
            r#"{"temperature":0,"chat_template_kwargs":{"enable_thinking":false}}"#.into();
        core.save_settings(
            None,
            &pebble_core::SettingsUpdate {
                chat: settings.chat,
                credential: pebble_core::CredentialChange::Keep,
                persona: None,
            },
        )
        .unwrap();
        let other = core.create_persona("Other").unwrap();
        let room = core
            .create_group("Observed", &[id.clone(), other.id.clone()])
            .unwrap();
        // Public observations without an admitted reply are real persona knowledge.
        let connection = db(root.path());
        let publish = |text: &str, sequence: i64| {
            let message = format!("public-{sequence}");
            connection.execute("INSERT INTO messages(id,conversation_id,sequence,sender_kind,sender_id,content,status) VALUES(?1,?2,?3,'user','owner',?4,'complete')",rusqlite::params![message,room.id,sequence,text]).unwrap();
            connection.execute("INSERT INTO persona_events(id,persona_id,sequence,conversation_id,message_id,kind) VALUES(?1,?2,?3,?4,?5,'received')",rusqlite::params![format!("event-{sequence}"),id,sequence,room.id,message]).unwrap();
        };
        publish("old eligible public knowledge", 1);
        let before = state(root.path(), &id);
        let task = core.start_fresh_context(&id).unwrap();
        let request = server.requests.recv().unwrap().1;
        assert_eq!(request["temperature"], 0);
        assert_eq!(request["chat_template_kwargs"]["enable_thinking"], false);
        let field = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        assert!(
            request[field]
                .to_string()
                .contains("old eligible public knowledge")
        );
        publish("new observation after frozen cutoff", 2);
        core.remove_group_member(&room.id, &id).unwrap();
        let status = wait(&task);
        assert_eq!(status.status, "completed", "{}", status.detail);
        assert_eq!(state(root.path(), &id), before);
        assert!(core.context_markers(&room.id).unwrap().is_empty());
        let persona = core
            .workspace()
            .unwrap()
            .personas
            .into_iter()
            .find(|p| p.id == id)
            .unwrap();
        assert_eq!(
            core.context_markers(&persona.conversation_id)
                .unwrap()
                .len(),
            1
        );
        let (chat, rx) = start(&core, &id, "continue privately");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let next = server.requests.recv().unwrap().1;
        assert_eq!(next["temperature"], 0);
        assert_eq!(next["chat_template_kwargs"]["enable_thinking"], false);
        assert!(
            next[field]
                .to_string()
                .contains("new observation after frozen cutoff")
        );
        assert!(
            !next[field]
                .to_string()
                .contains("old eligible public knowledge")
        );
        drop(chat);
    }
}
#[test]
fn maintenance_saved_effect_recovers_publication_failure_without_another_request() {
    let server = Server::memory(vec![
        stream(success("first")),
        stream(tool("chat_completions", "Consolidated once.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (chat, rx) = start(&core, &id, "old knowledge");
    terminal(&rx);
    server.requests.recv().unwrap();
    db(root.path()).execute_batch("CREATE TRIGGER reject_marker BEFORE INSERT ON context_markers BEGIN SELECT RAISE(ABORT,'synthetic marker failure'); END;").unwrap();
    let task = core.start_fresh_context(&id).unwrap();
    let failed = wait(&task);
    assert_eq!(failed.status, "failed");
    assert!(failed.memory_saved);
    assert!(failed.detail.contains("without repeating"));
    server.requests.recv().unwrap();
    assert_eq!(memory(&core, &id), "Consolidated once.");
    let persona = core.workspace().unwrap().personas.remove(0);
    assert!(
        core.context_markers(&persona.conversation_id)
            .unwrap()
            .is_empty()
    );
    drop(task);
    drop(chat);
    drop(core);
    let core = reopen(root.path());
    db(root.path())
        .execute_batch("DROP TRIGGER reject_marker;")
        .unwrap();
    let task = core.start_fresh_context(&id).unwrap();
    assert_eq!(wait(&task).status, "completed");
    assert_eq!(
        core.context_markers(&persona.conversation_id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db(root.path())
            .query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(task);
    drop(core);
    let core = reopen(root.path());
    assert_eq!(
        core.context_markers(&persona.conversation_id)
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn maintenance_cancel_keeps_context_and_other_personas_available() {
    let server = Server::memory(vec![
        stream(success("seed")),
        Plan {
            status: 200,
            body: tool("chat_completions", "must not save"),
            delay_headers: true,
            delay_tail: false,
        },
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (chat, rx) = start(&core, &id, "seed");
    terminal(&rx);
    server.requests.recv().unwrap();
    let before = memory(&core, &id);
    let task = core.start_fresh_context(&id).unwrap();
    server.requests.recv().unwrap();
    assert!(matches!(
        core.start_fresh_context(&id),
        Err(pebble_core::Error::Busy)
    ));
    task.cancel();
    assert_eq!(wait(&task).status, "cancelled");
    assert_eq!(memory(&core, &id), before);
    let persona = core.workspace().unwrap().personas.remove(0);
    assert!(
        core.context_markers(&persona.conversation_id)
            .unwrap()
            .is_empty()
    );
    drop(chat);
}
#[test]
fn exact_ordinary_headroom_boundary_and_empty_demo_transition() {
    let budget = pebble_core::ContextBudget {
        context_len: 262144,
        max_output_tokens: 4096,
    };
    assert_eq!(budget.maintenance_headroom(), 87381);
    let (_root, core, id) = isolated();
    let persona = core.workspace().unwrap().personas.remove(0);
    let old = persona.memory;
    let task = core.start_fresh_context(&id).unwrap();
    let status = wait(&task);
    assert_eq!(status.status, "completed");
    assert!(status.detail.contains("no chat model"));
    assert_eq!(memory(&core, &id), old);
    assert_eq!(
        core.context_markers(&persona.conversation_id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
#[ignore = "requires explicitly selected synthetic live endpoint"]
fn opt_in_live_memory_and_maintenance_both_native_protocols() {
    let endpoint = std::env::var("PEBBLE_LIVE_API_ROOT").expect("explicit live API root");
    let model = std::env::var("PEBBLE_LIVE_MODEL").expect("explicit live model");
    for api in ["chat_completions", "responses"] {
        let (root, core, id) = isolated();
        std::fs::write(root.path().join("config.toml"),format!("[defaults.chat]\nmodel='{model}'\nbase_url='{endpoint}'\napi='{api}'\napi_key=''\ncontext_len=262144\nmax_output_tokens=4096\n{}",if api=="responses"{"responses_schema_format='response_format'\n"}else{""})).unwrap();
        let persona = core.workspace().unwrap().personas.remove(0);
        std::fs::write(Path::new(&persona.directory).join("SOUL.md"),"You are a synthetic memory integration character. Follow explicit requests to use update_memory, preserving all existing synthetic memory. Always finish conversational turns with valid reply/state JSON; use native tools before the final reply when asked.").unwrap();
        let (chat, rx) = start(
            &core,
            &id,
            "This is an isolated synthetic test. Please use the native update_memory tool now: preserve current MEMORY and add the permanent synthetic preference that my favorite invented color is copper-blue. After the successful tool result, give a short conversational confirmation in the required reply/state JSON.",
        );
        let event = terminal(&rx).pop().unwrap();
        assert_eq!(
            event.kind,
            ChatEventKind::Completed,
            "{api}: {}",
            event.text
        );
        assert!(
            memory(&core, &id).contains("copper"),
            "{api}: model did not save the synthetic preference"
        );
        let before = state(root.path(), &id);
        let old = core.model_request(&chat.id).unwrap();
        let task = core.start_fresh_context(&id).unwrap();
        let status = wait_live(&task);
        assert_eq!(status.status, "completed", "{api}: {}", status.detail);
        assert_eq!(state(root.path(), &id), before);
        let (next, rx) = start(
            &core,
            &id,
            "What is my favorite invented color? Reply concisely using the required reply/state JSON.",
        );
        let event = terminal(&rx).pop().unwrap();
        assert_eq!(
            event.kind,
            ChatEventKind::Completed,
            "{api}: {}",
            event.text
        );
        let request = core.model_request(&next.id).unwrap();
        assert_ne!(request.session_id, old.session_id);
        assert!(String::from_utf8_lossy(&request.items[0].payload).contains("copper"));
        assert_eq!(
            core.context_markers(&persona.conversation_id)
                .unwrap()
                .len(),
            1
        );
        println!(
            "Synthetic live {api}: native memory tool + validated final + maintenance + continued chat passed; memory operations={}",
            db(root.path())
                .query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap()
        );
    }
}
fn wait_live(task: &Arc<MaintenanceTask>) -> pebble_core::MaintenanceStatus {
    for _ in 0..2000 {
        let s = task.status();
        if s.status != "running" {
            return s;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("live maintenance timed out")
}

#[test]
fn cancellation_after_memory_commit_preserves_effect_and_reports_partial_outcome() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "saved before cancellation")),
        Plan {
            status: 200,
            body: success("not accepted"),
            delay_headers: true,
            delay_tail: false,
        },
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (task, rx) = start(&core, &id, "remember");
    server.requests.recv().unwrap();
    server.requests.recv().unwrap();
    assert_eq!(memory(&core, &id), "saved before cancellation");
    task.cancel();
    let event = terminal(&rx).pop().unwrap();
    assert_eq!(event.kind, ChatEventKind::Cancelled);
    let reply = core.messages(&id).unwrap().last().unwrap().clone();
    assert!(reply.content.is_empty());
    assert!(reply.failure_detail.contains("Memory was saved"));
    assert_eq!(
        serde_json::from_str::<Value>(&state(root.path(), &id)).unwrap()["affinity_user"],
        20
    );
}

struct Capture(std::sync::mpsc::Sender<pebble_core::ChatEvent>);
impl pebble_core::ChatListener for Capture {
    fn on_event(&self, event: pebble_core::ChatEvent) -> Result<(), String> {
        self.0.send(event).map_err(|e| e.to_string())
    }
}
#[test]
fn normal_group_tool_calls_stay_persona_scoped_and_never_fan_out_native_results() {
    for api in ["chat_completions", "responses"] {
        let server = Server::memory(vec![
            stream(tool(api, "Private synthetic memory value.")),
            stream(if api == "responses" {
                responses_success("public acknowledgment")
            } else {
                success("public acknowledgment")
            }),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let other = core.create_persona("Listener").unwrap();
        let old = other.memory.clone();
        let room = core
            .create_group("Memory scope", &[id.clone(), other.id.clone()])
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let submission = core
            .send_room_message(
                &room.id,
                "Please remember this synthetic detail",
                std::slice::from_ref(&id),
                Arc::new(Capture(tx)),
            )
            .unwrap();
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        assert_eq!(memory(&core, &id), "Private synthetic memory value.");
        assert_eq!(memory(&core, &other.id), old);
        assert_eq!(core.room_messages(&room.id).unwrap().len(), 2);
        let seen:Vec<String>=db(root.path()).prepare("SELECT m.content FROM persona_events e JOIN messages m ON m.id=e.message_id WHERE e.persona_id=?1 ORDER BY e.sequence").unwrap().query_map([&other.id],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(
            seen.iter()
                .all(|s| !s.contains("Private synthetic memory value")
                    && !s.contains("memory-1")
                    && !s.contains("tool reasoning"))
        );
        drop(submission);
    }
}
#[test]
fn maintenance_rejects_changed_api_root_before_exporting_native_history() {
    let server = Server::memory(vec![stream(success("seed"))]);
    let (root, core, id) = isolated();
    configure(
        root.path(),
        &server.url,
        "same-model",
        "chat_completions",
        "",
    );
    let (_task, rx) = start(&core, &id, "private outgoing knowledge");
    terminal(&rx);
    server.requests.recv().unwrap();
    let old = db(root.path())
        .query_row(
            "SELECT id FROM model_sessions WHERE status='active'",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap();
    configure(
        root.path(),
        "http://127.0.0.1:9/v1",
        "same-model",
        "chat_completions",
        "",
    );
    let result = core.start_fresh_context(&id);
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("Connection changed")
    );
    assert_eq!(
        db(root.path())
            .query_row(
                "SELECT id FROM model_sessions WHERE status='active'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        old
    );
}

#[test]
fn final_publication_failure_reports_committed_memory_without_accepting_state() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "saved before final publication")),
        stream(success("not published")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let before = state(root.path(), &id);
    db(root.path()).execute_batch("CREATE TRIGGER reject_final_state BEFORE UPDATE OF status ON model_calls WHEN NEW.status='completed' BEGIN SELECT RAISE(FAIL,'synthetic final publication failure'); END;").unwrap();
    let (_task, rx) = start(&core, &id, "save synthetic fact");
    let event = terminal(&rx).pop().unwrap();
    assert_eq!(event.kind, ChatEventKind::Failed);
    assert!(event.text.contains("Memory was saved"));
    assert_eq!(memory(&core, &id), "saved before final publication");
    assert_eq!(state(root.path(), &id), before);
    let messages = core.messages(&id).unwrap();
    assert!(messages.last().unwrap().content.is_empty());
    assert!(
        messages
            .last()
            .unwrap()
            .failure_detail
            .contains("Memory was saved")
    );
}

#[test]
fn stale_saved_maintenance_is_superseded_but_new_observations_alone_keep_recovery() {
    for api in ["chat_completions", "responses"] {
        for change in [
            "same_state_chat",
            "changed_state_chat",
            "manual_memory",
            "observation_only",
        ] {
            let ordinary = |text: &str| {
                if api == "responses" {
                    responses_support::candidate(text)
                } else {
                    candidate(text)
                }
            };
            let mut plans = vec![
                stream(ordinary(&structured("seed"))),
                stream(tool(api, "Saved once.")),
            ];
            if change.ends_with("chat") {
                let output = if change == "changed_state_chat" {
                    json!({"reply":"newly accepted native output","state":{"affinity_user":37,"mood":"curious"}}).to_string()
                } else {
                    structured("newly accepted native output")
                };
                plans.push(stream(ordinary(&output)));
            }
            if change != "observation_only" {
                plans.push(stream(tool(
                    api,
                    "Saved once. Newly consolidated knowledge.",
                )));
            } else {
                plans.push(stream(ordinary(&structured("continued"))));
            }
            let server = Server::memory(plans);
            let (root, core, id) = isolated();
            configure(root.path(), &server.url, "mock", api, "");
            let (_seed, rx) = start(&core, &id, "old source knowledge");
            assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
            server.requests.recv().unwrap();
            db(root.path()).execute_batch("CREATE TRIGGER reject_stale_marker BEFORE INSERT ON context_markers BEGIN SELECT RAISE(ABORT,'synthetic marker failure'); END;").unwrap();
            let first = core.start_fresh_context(&id).unwrap();
            assert_eq!(wait(&first).status, "failed");
            server.requests.recv().unwrap();
            let cutoff: i64 = db(root.path())
                .query_row(
                    "SELECT event_cutoff FROM context_maintenance WHERE id=?1",
                    [&first.id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(memory(&core, &id), "Saved once.");
            if change.ends_with("chat") {
                let (_chat, rx) = start(&core, &id, "new knowledge since failed boundary");
                assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
                server.requests.recv().unwrap();
            } else if change == "manual_memory" {
                let persona = core.workspace().unwrap().personas.remove(0);
                std::fs::write(
                    Path::new(&persona.directory).join("MEMORY.md"),
                    "Saved once. Manual addition.",
                )
                .unwrap();
            } else {
                let room = core
                    .create_group("New observation", std::slice::from_ref(&id))
                    .unwrap();
                let connection = db(root.path());
                connection.execute("INSERT INTO messages(id,conversation_id,sequence,sender_kind,sender_id,content,status) VALUES('new-public',?1,1,'user','owner','post-cutoff observation','complete')",[&room.id]).unwrap();
                connection.execute("INSERT INTO persona_events(id,persona_id,sequence,conversation_id,message_id,kind) VALUES('new-event',?1,?2,?3,'new-public','received')",rusqlite::params![id,cutoff+1,room.id]).unwrap();
            }
            db(root.path())
                .execute_batch("DROP TRIGGER reject_stale_marker")
                .unwrap();
            let next = core.start_fresh_context(&id).unwrap();
            assert_eq!(wait(&next).status, "completed", "{api}/{change}");
            if change == "observation_only" {
                assert_eq!(next.id, first.id);
                assert!(server.requests.try_recv().is_err());
                let (_chat, rx) = start(&core, &id, "consume remaining observation");
                assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
                assert!(
                    server
                        .requests
                        .recv()
                        .unwrap()
                        .1
                        .to_string()
                        .contains("post-cutoff observation")
                );
            } else {
                assert_ne!(next.id, first.id);
                let request = server.requests.recv().unwrap().1;
                let text = request.to_string();
                assert!(text.contains("Saved once."));
                if change.ends_with("chat") {
                    assert!(text.contains("newly accepted native output"));
                } else {
                    assert!(text.contains("Manual addition"));
                }
                let prior: String = db(root.path())
                    .query_row(
                        "SELECT status FROM context_maintenance WHERE id=?1",
                        [&first.id],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(prior, "superseded");
            }
            let count: i64 = db(root.path())
                .query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, if change == "observation_only" { 1 } else { 2 });
        }
    }
}

#[test]
fn maintenance_publication_revalidates_binding_for_active_and_silent_first_contexts() {
    for api in ["chat_completions", "responses"] {
        for first_session in [false, true] {
            for change in ["base_url", "model", "protocol", "limits_credentials"] {
                let mut plans = Vec::new();
                if !first_session {
                    plans.push(stream(if api == "responses" {
                        responses_support::success("seed")
                    } else {
                        success("seed")
                    }));
                }
                plans.push(Plan {
                    status: 200,
                    body: tool(api, "Saved by original connection."),
                    delay_headers: true,
                    delay_tail: false,
                });
                let server = Server::memory(plans);
                let (root, core, id) = isolated();
                configure(root.path(), &server.url, "original-model", api, "");
                if first_session {
                    let room = core
                        .create_group("Silent knowledge", std::slice::from_ref(&id))
                        .unwrap();
                    let connection = db(root.path());
                    connection.execute("INSERT INTO messages(id,conversation_id,sequence,sender_kind,sender_id,content,status) VALUES('silent-source',?1,1,'user','owner','silent public knowledge','complete')",[&room.id]).unwrap();
                    connection.execute("INSERT INTO persona_events(id,persona_id,sequence,conversation_id,message_id,kind) VALUES('silent-event',?1,1,?2,'silent-source','received')",rusqlite::params![id,room.id]).unwrap();
                } else {
                    let (_seed, rx) = start(&core, &id, "native source knowledge");
                    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
                    server.requests.recv().unwrap();
                }
                let active = || {
                    db(root.path())
                        .query_row(
                            "SELECT id FROM model_sessions WHERE persona_id=?1 AND status='active'",
                            [&id],
                            |r| r.get::<_, String>(0),
                        )
                        .ok()
                };
                let before = active();
                let task = core.start_fresh_context(&id).unwrap();
                server.requests.recv().unwrap();
                let path = root.path().join("config.toml");
                let source = std::fs::read_to_string(&path).unwrap();
                let revised = match change {
                    "base_url" => source.replace(&server.url, "http://127.0.0.1:9/v1"),
                    "model" => source.replace("original-model", "changed-model"),
                    "protocol" => source.replace(
                        &format!("api='{api}'"),
                        &format!(
                            "api='{}'",
                            if api == "responses" {
                                "chat_completions"
                            } else {
                                "responses"
                            }
                        ),
                    ),
                    _ => source
                        .replace("context_len=65536", "context_len=131072")
                        .replace("max_output_tokens=4096", "max_output_tokens=8192")
                        .replace("api_key=''", "api_key='synthetic-new-key'"),
                };
                std::fs::write(path, revised).unwrap();
                let status = wait(&task);
                assert!(
                    status.memory_saved,
                    "{api}/{first_session}/{change}: {}",
                    status.detail
                );
                assert_eq!(memory(&core, &id), "Saved by original connection.");
                let persona = core.workspace().unwrap().personas.remove(0);
                if change == "limits_credentials" {
                    assert_eq!(status.status, "completed", "{}", status.detail);
                    assert_eq!(
                        core.context_markers(&persona.conversation_id)
                            .unwrap()
                            .len(),
                        1
                    );
                } else {
                    assert_eq!(status.status, "failed", "{}", status.detail);
                    assert!(status.detail.contains("Connection changed during"));
                    assert_eq!(active(), before);
                    assert!(
                        core.context_markers(&persona.conversation_id)
                            .unwrap()
                            .is_empty()
                    );
                    let created: i64 = db(root.path())
                        .query_row(
                            "SELECT COUNT(*) FROM model_sessions WHERE context_reason='memory'",
                            [],
                            |r| r.get(0),
                        )
                        .unwrap();
                    assert_eq!(created, 0);
                }
            }
        }
    }
}

#[test]
fn retry_eligibility_tracks_running_maintenance_without_blocking_other_personas() {
    let server = Server::memory(vec![
        Plan {
            status: 500,
            body: String::new(),
            delay_headers: false,
            delay_tail: false,
        },
        Plan {
            status: 200,
            body: tool("chat_completions", "not saved after cancel"),
            delay_headers: true,
            delay_tail: false,
        },
    ]);
    let (root, core, id) = isolated();
    let other = core.create_persona("Other").unwrap();
    let (other_call, other_rx) = start(&core, &other.id, "cancelled unrelated reply");
    other_call.cancel();
    terminal(&other_rx);
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (_failed, rx) = start(&core, &id, "retryable failed reply");
    terminal(&rx);
    server.requests.recv().unwrap();
    assert!(core.messages(&id).unwrap().last().unwrap().can_retry);
    assert!(core.messages(&other.id).unwrap().last().unwrap().can_retry);
    let task = core.start_fresh_context(&id).unwrap();
    server.requests.recv().unwrap();
    assert!(!core.messages(&id).unwrap().last().unwrap().can_retry);
    assert!(core.messages(&other.id).unwrap().last().unwrap().can_retry);
    task.cancel();
    assert_eq!(wait(&task).status, "cancelled");
    assert!(core.messages(&id).unwrap().last().unwrap().can_retry);
}

#[test]
fn unchanged_completed_review_exposes_partial_boundary_and_recovers_without_http() {
    for api in ["chat_completions", "responses"] {
        for interrupted in [false, true] {
            let first = if api == "responses" {
                responses_success("first")
            } else {
                success("first")
            };
            let server = Server::memory(vec![
                stream(first),
                stream(tool(api, "Synthetic test memory only.")),
            ]);
            let (root, core, id) = isolated();
            configure(root.path(), &server.url, "mock", api, "");
            let (chat, rx) = start(&core, &id, "old knowledge");
            terminal(&rx);
            server.requests.recv().unwrap();
            db(root.path()).execute_batch("CREATE TRIGGER reject_nochange_marker BEFORE INSERT ON context_markers BEGIN SELECT RAISE(ABORT,'synthetic no-change boundary failure'); END;").unwrap();
            let task = core.start_fresh_context(&id).unwrap();
            let failed = wait(&task);
            assert_eq!(failed.status, "failed");
            assert!(failed.memory_reviewed);
            assert!(!failed.memory_saved);
            assert!(failed.detail.contains("reviewed and unchanged"));
            assert!(failed.detail.contains("without another model request"));
            server.requests.recv().unwrap();
            if interrupted {
                db(root.path())
                    .execute(
                        "UPDATE context_maintenance SET status='running',detail='' WHERE id=?1",
                        [&failed.id],
                    )
                    .unwrap();
            }
            drop(task);
            drop(chat);
            drop(core);
            let core = reopen(root.path());
            let restored = core.maintenance_status(&id).unwrap().unwrap();
            assert!(restored.memory_reviewed);
            assert!(!restored.memory_saved);
            assert!(restored.detail.contains("reviewed and unchanged"));
            if interrupted {
                assert_eq!(restored.status, "interrupted");
            }
            db(root.path())
                .execute_batch("DROP TRIGGER reject_nochange_marker")
                .unwrap();
            let task = core.start_fresh_context(&id).unwrap();
            assert_eq!(task.id, failed.id);
            let completed = wait(&task);
            assert_eq!(completed.status, "completed");
            assert!(completed.memory_reviewed);
            assert!(!completed.memory_saved);
            assert!(
                server
                    .requests
                    .recv_timeout(Duration::from_millis(50))
                    .is_err()
            );
            assert_eq!(
                db(root.path())
                    .query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                core.context_markers(&core.workspace().unwrap().personas[0].conversation_id)
                    .unwrap()
                    .len(),
                1
            );
        }
    }
}

#[test]
fn public_room_stop_and_member_removal_preserve_saved_memory_notice_through_restart() {
    for api in ["chat_completions", "responses"] {
        for action in ["private_stop", "group_stop", "remove_member"] {
            let late = json!({"reply":"late final must not publish","state":{"affinity_user":99,"mood":"calm"}}).to_string();
            let final_output = if api == "responses" {
                responses_support::candidate(&late)
            } else {
                candidate(&late)
            };
            let server = Server::memory(vec![
                stream(tool(api, "saved before public cancellation")),
                Plan {
                    status: 200,
                    body: final_output,
                    delay_headers: true,
                    delay_tail: false,
                },
            ]);
            let (root, core, id) = isolated();
            configure(root.path(), &server.url, "mock", api, "");
            let room = if action == "private_stop" {
                core.workspace().unwrap().personas[0]
                    .conversation_id
                    .clone()
            } else {
                core.create_group("Cancellation memory", std::slice::from_ref(&id))
                    .unwrap()
                    .id
            };
            let (tx, rx) = std::sync::mpsc::channel();
            let mentions = if action == "private_stop" {
                vec![]
            } else {
                vec![id.clone()]
            };
            let submission = core
                .send_room_message(
                    &room,
                    "Remember this synthetic detail",
                    &mentions,
                    Arc::new(Capture(tx)),
                )
                .unwrap();
            server
                .requests
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            server
                .requests
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            let before = core.room_messages(&room).unwrap().pop().unwrap();
            let call: String = db(root.path())
                .query_row(
                    "SELECT id FROM model_calls WHERE reply_message_id=?1",
                    [&before.id],
                    |r| r.get(0),
                )
                .unwrap();
            let item_count: i64 = db(root.path())
                .query_row(
                    "SELECT COUNT(*) FROM model_items WHERE call_id=?1",
                    [&call],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(memory(&core, &id), "saved before public cancellation");
            if action == "remove_member" {
                core.remove_group_member(&room, &id).unwrap();
            } else {
                core.stop_room(&room).unwrap();
            }
            let immediate = core.room_messages(&room).unwrap().pop().unwrap();
            assert_eq!(immediate.status, pebble_core::MessageStatus::Cancelled);
            assert!(
                immediate.failure_detail.contains("Memory was saved"),
                "{api}/{action}: {}",
                immediate.failure_detail
            );
            let event = terminal(&rx).pop().unwrap();
            assert_eq!(event.kind, ChatEventKind::Cancelled);
            assert!(
                event
                    .messages
                    .last()
                    .unwrap()
                    .failure_detail
                    .contains("Memory was saved")
            );
            let assert_saved = |core: &Core| {
                let reply = core.room_messages(&room).unwrap().pop().unwrap();
                assert_eq!(reply.status, pebble_core::MessageStatus::Cancelled);
                assert!(reply.content.is_empty());
                assert!(reply.failure_detail.contains("Memory was saved"));
                assert!(reply.failure_detail.contains("not be repeated"));
                assert_eq!(memory(core, &id), "saved before public cancellation");
                assert_eq!(
                    serde_json::from_str::<Value>(&state(root.path(), &id)).unwrap()["affinity_user"],
                    20
                );
                assert_eq!(
                    db(root.path())
                        .query_row(
                            "SELECT COUNT(*) FROM model_items WHERE call_id=?1",
                            [&call],
                            |r| r.get::<_, i64>(0)
                        )
                        .unwrap(),
                    item_count
                );
                assert_eq!(
                    db(root.path())
                        .query_row("SELECT status FROM model_calls WHERE id=?1", [&call], |r| r
                            .get::<_, String>(0))
                        .unwrap(),
                    "cancelled"
                );
                assert_eq!(
                    db(root.path())
                        .query_row(
                            "SELECT COUNT(*) FROM memory_operations WHERE owner_id=?1",
                            [&call],
                            |r| r.get::<_, i64>(0)
                        )
                        .unwrap(),
                    1
                );
            };
            assert_saved(&core);
            drop(submission);
            drop(core);
            let core = reopen(root.path());
            assert_saved(&core);
        }
    }
}
