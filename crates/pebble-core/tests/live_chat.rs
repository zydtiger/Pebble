mod live_support;
use live_support::*;
use pebble_core::{ChatEventKind, MessageStatus};
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};

#[test]
fn live_stream_replays_native_assistant_and_frozen_prefix_after_restart() {
    let server = Server::new(vec![stream(success("你好 🪨")), stream(success("second"))]);
    let (root, core, id) = isolated();
    configure(
        root.path(),
        &format!("{}/", server.url),
        "mock-model",
        "chat_completions",
        "test-secret",
    );
    let (task, rx) = start(&core, &id, "first synthetic question");
    let events = terminal(&rx);
    assert_eq!(events.last().unwrap().kind, ChatEventKind::Completed);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == ChatEventKind::TextDelta)
            .map(|e| e.text.as_str())
            .collect::<String>(),
        "你好 🪨"
    );
    let first = core.model_request(&task.id).unwrap();
    let (headers, body) = server.requests.recv().unwrap();
    assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(
        headers
            .to_lowercase()
            .contains("authorization: bearer test-secret")
    );
    assert_eq!(body["model"], "mock-model");
    assert_eq!(body["stream"], true);
    let saved = core.messages(&id).unwrap();
    drop(task);
    drop(core);
    let core = reopen(root.path());
    assert_eq!(saved, core.messages(&id).unwrap());
    let persona = core.workspace().unwrap().personas.remove(0);
    std::fs::write(
        Path::new(&persona.directory).join("SOUL.md"),
        "changed test soul",
    )
    .unwrap();
    let (task, rx) = start(&core, &id, "second synthetic question");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    let second = core.model_request(&task.id).unwrap();
    assert_eq!(first.session_id, second.session_id);
    assert_eq!(second.items.len(), 4);
    for (a, b) in first.items.iter().zip(&second.items) {
        assert_eq!(a.payload, b.payload);
        assert_eq!(a.id, b.id);
    }
    let (_, body2) = server.requests.recv().unwrap();
    assert_eq!(
        body2["messages"][2],
        json!({"role":"assistant","content":"你好 🪨","reasoning_content":"private-test-reasoning"})
    );
    assert_eq!(body2["messages"][0], body["messages"][0]);
    assert!(!format!("{second:?}").contains("test-secret"));
}
#[test]
fn bad_streams_fail_durably_without_accepted_outputs() {
    let cases = [
        (503, "provider failure".into(), "HTTP 503"),
        (200, "data: {broken}\n\n".into(), "malformed JSON"),
        (
            200,
            event(json!({"content":"partial"}), Value::Null),
            "completion marker",
        ),
        (
            200,
            format!(
                "{}{}",
                event(json!({"content":"partial"}), Value::Null),
                event(json!({}), json!("length"))
            ),
            "output budget",
        ),
        (
            200,
            format!(
                "{}{}data: [DONE]\n\n",
                event(json!({"reasoning_content":"hidden"}), Value::Null),
                event(json!({}), json!("stop"))
            ),
            "without visible",
        ),
        (200, "data: [DONE]\n\n".into(), "finish reason"),
        (
            200,
            "data: {\"error\":{\"message\":\"test-secret\"}}\n\n".into(),
            "streaming error",
        ),
    ];
    for (status, body, expected) in cases {
        let server = Server::new(vec![Plan {
            status,
            body,
            delay_headers: false,
            delay_tail: false,
        }]);
        let (root, core, id) = isolated();
        configure(
            root.path(),
            &server.url,
            "mock-model",
            "chat_completions",
            "test-secret",
        );
        let (_task, rx) = start(&core, &id, "test failure");
        let events = terminal(&rx);
        let last = events.last().unwrap();
        assert_eq!(last.kind, ChatEventKind::Failed, "{expected}");
        assert!(last.text.contains(expected), "{}", last.text);
        assert!(!last.text.contains("test-secret"));
        let messages = core.messages(&id).unwrap();
        assert_eq!(messages[1].status, MessageStatus::Failed);
        if expected == "completion marker" || expected == "output budget" {
            assert_eq!(messages[1].content, "partial");
        }
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM model_items WHERE kind='output'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM persona_events WHERE kind='sent'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert!(
            db.query_row("SELECT error FROM model_calls", [], |r| r
                .get::<_, String>(0))
                .unwrap()
                .contains(expected)
        );
    }
}
#[test]
fn cancellation_interrupts_headers_and_idle_stream_and_omits_blank_auth() {
    for headers in [true, false] {
        let server = Server::new(vec![Plan {
            status: 200,
            body: success("partial"),
            delay_headers: headers,
            delay_tail: !headers,
        }]);
        let (root, core, id) = isolated();
        configure(
            root.path(),
            &server.url,
            "mock-model",
            "chat_completions",
            "",
        );
        let (task, rx) = start(&core, &id, "cancel test");
        let (request, _) = server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(!request.to_lowercase().contains("authorization:"));
        if !headers {
            loop {
                if rx.recv_timeout(Duration::from_secs(2)).unwrap().kind == ChatEventKind::TextDelta
                {
                    break;
                }
            }
        }
        let before = Instant::now();
        task.cancel();
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Cancelled);
        assert!(before.elapsed() < Duration::from_millis(300));
        assert_eq!(
            core.messages(&id).unwrap()[1].status,
            MessageStatus::Cancelled
        );
    }
}
#[test]
fn failed_responses_connection_and_missing_credentials_never_fall_back_to_demo() {
    let (root, core, id) = isolated();
    configure(
        root.path(),
        "http://127.0.0.1:9/v1",
        "mock",
        "responses",
        "",
    );
    let (_task, rx) = start(&core, &id, "no demo");
    let events = terminal(&rx);
    assert_eq!(events.last().unwrap().kind, ChatEventKind::Failed);
    assert!(events.last().unwrap().text.contains("Could not connect"));
    assert!(core.messages(&id).unwrap()[1].content.is_empty());
    std::fs::write(root.path().join("config.toml"),"[defaults.chat]\nmodel='mock'\nbase_url='http://127.0.0.1:9/v1'\napi='chat_completions'\napi_key_env='PEBBLE_INTENTIONALLY_MISSING_KEY_830204'\n").unwrap();
    let (_task, rx) = start(&core, &id, "missing credential");
    assert!(
        terminal(&rx)
            .last()
            .unwrap()
            .text
            .contains("environment variable is unavailable")
    );
}
#[test]
fn changed_connection_starts_fresh_context_and_preserves_transcript_and_v2_ids() {
    let (root, core, id) = isolated();
    let (demo, rx) = start(&core, &id, "old demo event");
    terminal(&rx);
    let old = core.model_request(&demo.id).unwrap();
    let saved = core.messages(&id).unwrap();
    drop(demo);
    drop(core);
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    db.execute_batch("ALTER TABLE model_calls DROP COLUMN retry_of; ALTER TABLE model_items DROP COLUMN decoded_length; ALTER TABLE model_sessions DROP COLUMN context_reason; ALTER TABLE model_sessions DROP COLUMN base_url; ALTER TABLE model_sessions DROP COLUMN event_start_sequence; PRAGMA user_version=2;").unwrap();
    let core = reopen(root.path());
    assert_eq!(saved, core.messages(&id).unwrap());
    let server = Server::new(vec![stream(success("live")), stream(success("changed"))]);
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    assert!(
        !core.workspace().unwrap().personas[0]
            .context_notice
            .is_empty()
    );
    let (live, rx) = start(&core, &id, "new live event");
    terminal(&rx);
    let request = core.model_request(&live.id).unwrap();
    assert_ne!(old.session_id, request.session_id);
    assert_eq!(request.items.len(), 2);
    assert!(
        request
            .items
            .iter()
            .all(|i| !String::from_utf8_lossy(&i.payload).contains("old demo event"))
    );
    configure(
        root.path(),
        &server.url,
        "other-model",
        "chat_completions",
        "",
    );
    let (other, rx) = start(&core, &id, "other event");
    terminal(&rx);
    assert_ne!(
        request.session_id,
        core.model_request(&other.id).unwrap().session_id
    );
    assert_eq!(core.model_request(&other.id).unwrap().items.len(), 2);
    drop(live);
    drop(other);
    drop(core);
    assert!(
        !reopen(root.path()).workspace().unwrap().personas[0]
            .context_notice
            .is_empty()
    );
}
