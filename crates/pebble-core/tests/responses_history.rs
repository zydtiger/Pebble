mod live_support;
mod responses_support;
use live_support::{Plan, Server, configure, isolated, reopen, start, stream, terminal};
use pebble_core::{ChatEventKind, MessageStatus};
use responses_support::{items, success};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn native_multiple_items_and_opaque_state_replay_after_restart_without_rewriting_prefix() {
    let server = Server::new(vec![stream(success("你好 🪨")), stream(success("second"))]);
    let (root, core, id) = isolated();
    configure(
        root.path(),
        &format!("{}/", server.url),
        "responses-model",
        "responses",
        "test-secret",
    );
    let (task, rx) = start(&core, &id, "first synthetic input");
    let events = terminal(&rx);
    assert_eq!(
        events.last().unwrap().kind,
        ChatEventKind::Completed,
        "{}",
        events.last().unwrap().text
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == ChatEventKind::TextDelta)
            .map(|e| e.text.as_str())
            .collect::<String>(),
        "你好 🪨"
    );
    let first = core.model_request(&task.id).unwrap();
    let (headers, request) = server.requests.recv().unwrap();
    assert!(headers.starts_with("POST /v1/responses HTTP/1.1"));
    assert!(
        headers
            .to_lowercase()
            .contains("authorization: bearer test-secret")
    );
    assert_eq!(request["model"], "responses-model");
    assert_eq!(request["stream"], true);
    assert_eq!(request["store"], false);
    assert_eq!(request["max_output_tokens"], 4096);
    assert_eq!(request["include"], json!(["reasoning.encrypted_content"]));
    assert!(request.get("previous_response_id").is_none());
    assert!(request.get("messages").is_none());
    assert_eq!(request["input"][0]["content"][0]["type"], "input_text");
    let saved = core.messages(&id).unwrap();
    drop(task);
    drop(core);
    let core = reopen(root.path());
    assert_eq!(core.messages(&id).unwrap(), saved);
    let persona = core.workspace().unwrap().personas.remove(0);
    std::fs::write(
        std::path::Path::new(&persona.directory).join("SOUL.md"),
        "edited after native output",
    )
    .unwrap();
    let (task, rx) = start(&core, &id, "second synthetic input");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    let second = core.model_request(&task.id).unwrap();
    assert_eq!(second.items.len(), 5);
    assert_eq!(second.session_id, first.session_id);
    for (a, b) in first.items.iter().zip(&second.items) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.payload, b.payload);
    }
    let (_, request2) = server.requests.recv().unwrap();
    assert_eq!(request2["input"][0], request["input"][0]);
    assert_eq!(
        &request2["input"].as_array().unwrap()[2..4],
        items("你好 🪨")
    );
    assert!(!format!("{second:?}").contains("test-secret"));
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM persona_events WHERE kind='sent'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(db.query_row("SELECT COUNT(*) FROM model_item_events x JOIN model_items i ON i.id=x.item_id WHERE i.kind='output'",[],|r|r.get::<_,i64>(0)).unwrap(),4);
}

#[test]
fn queued_responses_followup_includes_predecessors_complete_native_output() {
    let server = Server::new(vec![
        Plan {
            status: 200,
            body: success("first"),
            delay_headers: false,
            delay_tail: true,
        },
        stream(success("second")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "responses", "");
    let (first, rx) = start(&core, &id, "first input");
    server
        .requests
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let (queued, qrx) = start(&core, &id, "queued followup");
    assert_eq!(core.messages(&id).unwrap()[3].status, MessageStatus::Queued);
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    assert_eq!(
        terminal(&qrx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    let first = core.model_request(&first.id).unwrap();
    let next = core.model_request(&queued.id).unwrap();
    assert_eq!(first.items.len(), 2);
    assert_eq!(next.items.len(), 5);
    assert_eq!(
        serde_json::from_slice::<Value>(&next.items[2].payload).unwrap(),
        items("first")[0]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&next.items[3].payload).unwrap(),
        items("first")[1]
    );
}

#[test]
fn multi_item_publication_rolls_back_if_later_native_item_fails() {
    let server = Server::new(vec![stream(success("visible partial"))]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "responses", "");
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    db.execute_batch("CREATE TRIGGER reject_message_item BEFORE INSERT ON model_items WHEN NEW.kind='output' AND json_extract(CAST(NEW.payload AS TEXT), '$.type')='message' BEGIN SELECT RAISE(ABORT, 'synthetic later item failure'); END;").unwrap();
    let (_task, rx) = start(&core, &id, "publication failure");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Failed);
    let saved = core.messages(&id).unwrap();
    assert_eq!(saved[1].content, "visible partial");
    assert_eq!(saved[1].status, MessageStatus::Failed);
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
}
