mod live_support;
mod migration_support;
mod responses_support;
use live_support::*;
use pebble_core::{ChatEventKind, MessageStatus};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

fn db(root: &Path) -> Connection {
    Connection::open(root.join("data/app.sqlite")).unwrap()
}
fn state(root: &Path, persona: &str) -> Value {
    let text: String = db(root)
        .query_row(
            "SELECT current_state FROM persona_states WHERE persona_id=?1",
            [persona],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_str(&text).unwrap()
}
fn next(reply: &str, affinity: u64, mood: &str) -> String {
    json!({"reply":reply,"state":{"affinity_user":affinity,"mood":mood}}).to_string()
}
fn body(api: &str, text: &str) -> String {
    if api == "responses" {
        responses_support::candidate(text)
    } else {
        candidate(text)
    }
}
fn no_publication(root: &Path, persona: &str) {
    assert_eq!(
        state(root, persona),
        json!({"affinity_user":20,"mood":"calm"})
    );
    let db = db(root);
    for sql in [
        "SELECT COUNT(*) FROM model_items WHERE kind='output'",
        "SELECT COUNT(*) FROM persona_events WHERE kind='sent'",
    ] {
        assert_eq!(db.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
}

#[test]
fn accepted_state_sequences_queued_calls_and_survives_restart_model_and_fresh_context() {
    for api in ["chat_completions", "responses"] {
        let server = Server::new(vec![
            Plan {
                status: 200,
                body: body(api, &next("first visible", 21, "curious")),
                delay_headers: false,
                delay_tail: true,
            },
            stream(body(api, &next("second visible", 22, "touched"))),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "first-model", api, "");
        let (first, rx) = start(&core, &id, "first user");
        let (_, first_http) = server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(core.messages(&id).unwrap()[1].content.is_empty());
        no_publication(root.path(), &id);
        let (queued, qrx) = start(&core, &id, "queued user");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let events = terminal(&qrx);
        assert_eq!(events.last().unwrap().kind, ChatEventKind::Completed);
        assert!(events.iter().all(|e| e.kind != ChatEventKind::TextDelta));
        let request = core.model_request(&queued.id).unwrap();
        assert_eq!(
            request.state_before,
            Some(json!({"affinity_user":21,"mood":"curious"}))
        );
        let input: Value = serde_json::from_slice(&request.items.last().unwrap().payload).unwrap();
        let content = if api == "responses" {
            input["content"][0]["text"].as_str().unwrap()
        } else {
            input["content"].as_str().unwrap()
        };
        let envelope: Value = serde_json::from_str(content).unwrap();
        assert_eq!(
            envelope["current_state"],
            request.state_before.clone().unwrap()
        );
        let (_, second_http) = server.requests.recv().unwrap();
        let field = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        assert_eq!(first_http[field][0], second_http[field][0]);
        assert!(first_http.get("response_format").is_none());
        assert!(first_http.get("text").is_none());
        assert_eq!(first_http["tool_choice"], "auto");

        assert_eq!(core.messages(&id).unwrap()[3].content, "second visible");
        assert_eq!(
            state(root.path(), &id),
            json!({"affinity_user":22,"mood":"touched"})
        );
        let prefix = request.items[0].payload.clone();
        drop(first);
        drop(queued);
        drop(core);
        let core = reopen(root.path());
        assert_eq!(
            state(root.path(), &id),
            json!({"affinity_user":22,"mood":"touched"})
        );
        // An explicit new context and model replacement carry the accepted state.
        core.fresh_context(&id).unwrap();
        let new = Server::new(vec![stream(body(api, &next("new model", 22, "touched")))]);
        configure(root.path(), &new.url, "replacement-model", api, "");
        let (task, rx) = start(&core, &id, "new context input");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let fresh = core.model_request(&task.id).unwrap();
        assert_ne!(fresh.session_id, request.session_id);
        assert_eq!(fresh.items.len(), 2);
        assert!(String::from_utf8_lossy(&fresh.items[0].payload).contains("touched"));
        assert_eq!(
            core.model_request(&request.call_id).unwrap().items[0].payload,
            prefix
        );
    }
}

#[test]
fn format_repairs_use_one_frozen_input_and_state_without_accepting_rejected_candidates() {
    for api in ["chat_completions", "responses"] {
        let server = Server::new(vec![
            stream(body(api, "{invalid-secret-candidate")),
            stream(body(api, &next("rejected-secret-reply", 101, "bad"))),
            stream(body(api, &next("accepted only", 21, "warm"))),
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
        let (task, rx) = start(&core, &id, "one user");
        let events = terminal(&rx);
        assert_eq!(
            events.last().unwrap().kind,
            ChatEventKind::Completed,
            "{}",
            events.last().unwrap().text
        );
        assert_eq!(events.len(), 2);
        assert_eq!(core.messages(&id).unwrap().len(), 2);
        assert_eq!(core.messages(&id).unwrap()[1].content, "accepted only");
        assert_eq!(
            state(root.path(), &id),
            json!({"affinity_user":21,"mood":"warm"})
        );
        let requests: Vec<Value> = (0..3).map(|_| server.requests.recv().unwrap().1).collect();
        let field = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        for (index, http) in requests.iter().enumerate() {
            assert_eq!(http["temperature"], 0);
            assert_eq!(http["chat_template_kwargs"]["enable_thinking"], false);
            assert_eq!(
                &http[field].as_array().unwrap()[..2],
                requests[0][field].as_array().unwrap()
            );
            assert_eq!(
                http[field].as_array().unwrap().len(),
                if index == 0 { 2 } else { 3 }
            );
            assert!(!http.to_string().contains("invalid-secret-candidate"));
            assert!(!http.to_string().contains("rejected-secret-reply"));
        }
        let frozen = core.model_request(&task.id).unwrap();
        assert_eq!(frozen.items.len(), 2);
        assert_eq!(
            frozen.state_before,
            Some(json!({"affinity_user":20,"mood":"calm"}))
        );
        let db = db(root.path());
        let errors: String = db
            .query_row("SELECT validation_errors FROM model_calls", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<String>>(&errors).unwrap().len(),
            2
        );
        assert!(!errors.contains("secret"));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM persona_events", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM model_items WHERE kind='output'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            if api == "responses" { 2 } else { 1 }
        );
    }
}

#[test]
fn repair_exhaustion_and_late_publication_failure_leave_all_accepted_state_unchanged() {
    for api in ["chat_completions", "responses"] {
        for rollback in [false, true] {
            let plans = if rollback {
                vec![stream(body(api, &next("must not be shown", 90, "excited")))]
            } else {
                (0..3)
                    .map(|_| stream(body(api, &next("invalid candidate", 200, "bad"))))
                    .collect()
            };
            let server = Server::new(plans);
            let (root, core, id) = isolated();
            configure(root.path(), &server.url, "mock", api, "");
            if rollback {
                db(root.path()).execute_batch("CREATE TRIGGER reject_sent BEFORE INSERT ON persona_events WHEN NEW.kind='sent' BEGIN SELECT RAISE(ABORT,'synthetic acceptance rollback'); END;").unwrap();
            }
            let (_task, rx) = start(&core, &id, "one user");
            let events = terminal(&rx);
            assert_eq!(events.last().unwrap().kind, ChatEventKind::Failed);
            assert!(events.iter().all(|e| e.kind != ChatEventKind::TextDelta));
            assert!(core.messages(&id).unwrap()[1].content.is_empty());
            no_publication(root.path(), &id);
            let errors: String = db(root.path())
                .query_row("SELECT validation_errors FROM model_calls", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Vec<String>>(&errors).unwrap().len(),
                if rollback { 0 } else { 3 }
            );
            if !rollback {
                assert!(events.last().unwrap().text.contains("after 3 candidates"));
            }
        }
    }
}

#[test]
fn cancellation_and_refusal_do_not_consume_format_repair_budget_or_state() {
    for api in ["chat_completions", "responses"] {
        let server = Server::new(vec![Plan {
            status: 200,
            body: body(api, &next("never visible", 90, "excited")),
            delay_headers: false,
            delay_tail: true,
        }]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let (task, rx) = start(&core, &id, "cancel pending");
        server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(core.messages(&id).unwrap()[1].content.is_empty());
        task.cancel();
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Cancelled);
        no_publication(root.path(), &id);
        assert_eq!(
            db(root.path())
                .query_row("SELECT validation_errors FROM model_calls", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "[]"
        );
    }
    let refusal = format!(
        "{}{}data: [DONE]\n\n",
        event(json!({"refusal":"synthetic refusal"}), Value::Null),
        event(json!({}), json!("stop"))
    );
    let server = Server::new(vec![stream(refusal)]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (_task, rx) = start(&core, &id, "refusal");
    let result = terminal(&rx);
    assert_eq!(result.last().unwrap().kind, ChatEventKind::Failed);
    assert!(result.last().unwrap().text.contains("refused"));
    no_publication(root.path(), &id);
    assert_eq!(
        db(root.path())
            .query_row("SELECT validation_errors FROM model_calls", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "[]"
    );
}

#[test]
fn schema_changes_keep_the_frozen_contract_and_reject_incompatible_fresh_context() {
    let server = Server::new(vec![
        stream(candidate(&next("first", 21, "a longer mood"))),
        stream(candidate(&next("still old contract", 21, "a longer mood"))),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (first, rx) = start(&core, &id, "first");
    terminal(&rx);
    let before = core.model_request(&first.id).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let path = Path::new(&persona.directory).join("response_schema.json");
    let mut schema = persona.response_schema;
    schema["properties"]["state"]["properties"]["mood"]["maxLength"] = json!(8);
    std::fs::write(path, serde_json::to_string(&schema).unwrap()).unwrap();
    let (second, rx) = start(&core, &id, "old session");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    assert_eq!(
        core.model_request(&second.id).unwrap().response_schema,
        before.response_schema
    );
    assert!(
        core.fresh_context(&id)
            .unwrap_err()
            .to_string()
            .contains("incompatible")
    );
    assert_eq!(state(root.path(), &id)["mood"], "a longer mood");
    assert_eq!(
        db(root.path())
            .query_row(
                "SELECT id FROM model_sessions WHERE status='active'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        before.session_id
    );
}

#[test]
fn v6_upgrade_preserves_plain_native_context_until_explicit_fresh_boundary() {
    let (root, core, id) = isolated();
    let (task, rx) = start(&core, &id, "legacy input");
    terminal(&rx);
    let request = core.model_request(&task.id).unwrap();
    let directory = core.workspace().unwrap().personas[0].directory.clone();
    drop(task);
    drop(core);
    let db = db(root.path());
    // A synthetic v0.1.0 native prefix/output with no structured contract.
    for (kind, value) in [
        (
            "prefix",
            json!({"role":"system","content":"Legacy frozen SOUL and MEMORY"}),
        ),
        (
            "output",
            json!({"role":"assistant","content":"Legacy plain reply","reasoning_content":"legacy opaque reasoning"}),
        ),
    ] {
        let payload = serde_json::to_vec(&value).unwrap();
        db.execute(
            "UPDATE model_items SET codec='json',payload=?2,decoded_length=?3 WHERE kind=?1",
            params![kind, payload, payload.len() as i64],
        )
        .unwrap();
    }
    raw_history(root.path());
    let old_items: Vec<(String, Vec<u8>)> = db
        .prepare("SELECT id,payload FROM model_items ORDER BY sequence")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    migration_support::v7(root.path());
    db.execute_batch("DROP TABLE persona_states; ALTER TABLE model_sessions DROP COLUMN response_schema; ALTER TABLE model_calls DROP COLUMN state_before; ALTER TABLE model_calls DROP COLUMN validation_errors; PRAGMA user_version=6;").unwrap();
    std::fs::remove_file(Path::new(&directory).join("response_schema.json")).unwrap();
    let legacy = "# keep this comment\n[persona]\ndisplay_name='Mira'\nsubtitle='legacy'\n[custom]\nuntouched='keep'\n";
    std::fs::write(Path::new(&directory).join("config.toml"), legacy).unwrap();
    let core = reopen(root.path());
    let persona = core.workspace().unwrap().personas.remove(0);
    assert!(persona.context_notice.contains("Legacy model context"));
    let config = std::fs::read_to_string(Path::new(&directory).join("config.toml")).unwrap();
    assert!(config.contains("# keep this comment"));
    assert!(config.contains("untouched='keep'"));
    let preserved = core.model_request(&request.call_id).unwrap();
    assert!(preserved.response_schema.is_none());
    assert_eq!(preserved.session_id, request.session_id);
    for (item, (id, payload)) in preserved.items.iter().zip(&old_items) {
        assert_eq!(&item.id, id);
        assert_eq!(&item.payload, payload);
    }
    let (legacy_task, rx) = start(&core, &id, "continue legacy");
    let events = terminal(&rx);
    assert_eq!(events.last().unwrap().kind, ChatEventKind::Completed);
    assert!(events.iter().any(|e| e.kind == ChatEventKind::TextDelta));
    assert!(
        core.model_request(&legacy_task.id)
            .unwrap()
            .response_schema
            .is_none()
    );
    assert_eq!(
        state(root.path(), &id),
        json!({"affinity_user":20,"mood":"calm"})
    );
    core.fresh_context(&id).unwrap();
    let (new, rx) = start(&core, &id, "new structured context");
    let events = terminal(&rx);
    assert_eq!(events.last().unwrap().kind, ChatEventKind::Completed);
    assert!(events.iter().all(|e| e.kind != ChatEventKind::TextDelta));
    assert!(
        core.model_request(&new.id)
            .unwrap()
            .response_schema
            .is_some()
    );
    for (item_id, payload) in old_items {
        assert_eq!(
            db.query_row(
                "SELECT payload FROM model_items WHERE id=?1",
                [item_id],
                |r| r.get::<_, Vec<u8>>(0)
            )
            .unwrap(),
            payload
        );
    }
    assert_eq!(
        core.messages(&id).unwrap()[1].status,
        MessageStatus::Complete
    );
}

#[test]
fn invalid_definition_initial_state_and_irrelevant_provider_selector_are_rejected() {
    let (root, core, id) = isolated();
    let persona = core.workspace().unwrap().personas.remove(0);
    let schema_path = Path::new(&persona.directory).join("response_schema.json");
    let initial =
        std::fs::read_to_string(Path::new(&persona.directory).join("config.toml")).unwrap();
    let mut schema = persona.response_schema.clone();
    schema["properties"]["state"]["required"] = json!(["mood"]);
    std::fs::write(&schema_path, serde_json::to_string(&schema).unwrap()).unwrap();
    assert!(
        core.workspace()
            .unwrap_err()
            .to_string()
            .contains("every declared state field required")
    );
    std::fs::write(
        &schema_path,
        serde_json::to_string(&persona.response_schema).unwrap(),
    )
    .unwrap();
    std::fs::write(
        Path::new(&persona.directory).join("config.toml"),
        initial.replace("affinity_user = 20", "affinity_user = 200"),
    )
    .unwrap();
    assert!(core.workspace().is_err());
    assert_eq!(state(root.path(), &id)["affinity_user"], 20);
    std::fs::write(Path::new(&persona.directory).join("config.toml"), initial).unwrap();
    std::fs::write(root.path().join("config.toml"),"[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='mock'\nbase_url='http://localhost:9/v1'\napi='chat_completions'\nresponses_schema_format='response_format'\n").unwrap();
    assert!(
        core.workspace()
            .unwrap_err()
            .to_string()
            .contains("applies only to responses")
    );
    std::fs::write(root.path().join("config.toml"),"[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='mock'\nbase_url='http://localhost:9/v1'\napi='responses'\nresponses_schema_format='response_format'\n").unwrap();
    let settings = core.settings(None).unwrap();
    core.save_settings(
        None,
        &pebble_core::SettingsUpdate {
            chat: settings.chat,
            credential: pebble_core::CredentialChange::Keep,
            persona: None,
        },
    )
    .unwrap();
    assert_eq!(
        core.workspace().unwrap().personas[0]
            .chat
            .as_ref()
            .unwrap()
            .responses_schema_format
            .as_deref(),
        Some("response_format")
    );
    std::fs::remove_file(schema_path.clone()).unwrap();
    drop(core);
    assert!(reopen_error(root.path()).contains("response_schema.json"));
    assert!(!schema_path.exists());
}
fn reopen_error(root: &Path) -> String {
    match pebble_core::Core::open(root) {
        Ok(_) => panic!("Invalid initialized root was silently reseeded"),
        Err(error) => error.to_string(),
    }
}

fn malformed_pending_legacy_contract_returns_configuration_error(contract: &str) {
    let (root, core, id) = isolated();
    let directory = core.workspace().unwrap().personas[0].directory.clone();
    let config_path = Path::new(&directory).join("config.toml");
    let schema_path = Path::new(&directory).join("response_schema.json");
    let original = std::fs::read_to_string(&config_path).unwrap();
    let schema = std::fs::read(&schema_path).unwrap();
    drop(core);
    db(root.path())
        .execute("DELETE FROM persona_states WHERE persona_id=?1", [&id])
        .unwrap();
    let malformed =
        format!("# preserve authored legacy values\n{contract}\n[persona]\ndisplay_name='Mira'\n");
    std::fs::write(&config_path, &malformed).unwrap();
    let result = std::panic::catch_unwind(|| pebble_core::Core::open(root.path()));
    assert!(result.is_ok(), "Invalid legacy contract must not panic");
    assert!(matches!(
        result.unwrap(),
        Err(pebble_core::Error::Configuration { .. })
    ));
    assert_eq!(std::fs::read_to_string(&config_path).unwrap(), malformed);
    assert_eq!(std::fs::read(&schema_path).unwrap(), schema);
    assert_eq!(
        db(root.path())
            .query_row("SELECT COUNT(*) FROM persona_states", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    std::fs::write(&config_path, original).unwrap();
    let repaired = reopen(root.path());
    assert_eq!(repaired.workspace().unwrap().personas[0].id, id);
    assert_eq!(state(root.path(), &id)["affinity_user"], 20);
}

#[test]
fn pending_legacy_scalar_response_returns_configuration_error_without_panic() {
    malformed_pending_legacy_contract_returns_configuration_error("response='legacy-custom-value'");
}

#[test]
fn pending_legacy_scalar_initial_state_returns_configuration_error_without_panic() {
    malformed_pending_legacy_contract_returns_configuration_error(
        "[response]\ninitial_state='invalid-state'",
    );
}
