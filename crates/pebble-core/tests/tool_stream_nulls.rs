mod live_support;
use live_support::*;
use pebble_core::ChatEventKind;
use serde_json::{Value, json};

#[test]
fn nullable_tool_fragments_save_memory_then_publish_and_replay_a_reply() {
    let arguments = json!({"content":"Synthetic preference: blue."}).to_string();
    let body = format!(
        "{}{}{}{}data: [DONE]\n\n",
        event(
            json!({"tool_calls":[{"index":0,"id":"call-nullable","type":"function","function":{"name":"update_memory","arguments":""}}]}),
            Value::Null
        ),
        event(
            json!({"tool_calls":[{"index":0,"id":null,"type":"function","function":{"name":null,"arguments":arguments}}]}),
            Value::Null
        ),
        event(
            json!({"tool_calls":[{"index":0,"id":null,"type":null,"function":{"name":null,"arguments":null}}]}),
            Value::Null
        ),
        event(json!({"reasoning_content":null}), json!("tool_calls")),
    );
    let server = Server::memory(vec![
        stream(body),
        stream(success("Saved your preference.")),
        stream(success("Still remembered.")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (_task, rx) = start(&core, &id, "Remember my synthetic preference.");
    let events = terminal(&rx);
    assert_eq!(
        events.last().unwrap().kind,
        ChatEventKind::Completed,
        "{events:?}"
    );
    assert_eq!(
        core.messages(&id).unwrap().last().unwrap().content,
        "Saved your preference."
    );
    assert_eq!(
        core.workspace().unwrap().personas[0].memory,
        "Synthetic preference: blue."
    );
    server.requests.recv().unwrap();
    let continuation = server.requests.recv().unwrap().1;
    let assistant = continuation["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m.get("tool_calls").is_some())
        .unwrap();
    assert_eq!(
        assistant["tool_calls"][0],
        json!({"id":"call-nullable","type":"function","function":{"name":"update_memory","arguments":arguments}})
    );
    assert!(
        continuation["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "tool" && m["tool_call_id"] == "call-nullable")
    );
    let (_task, rx) = start(&core, &id, "Continue.");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    let replay = server.requests.recv().unwrap().1;
    assert!(replay["messages"].as_array().unwrap().contains(assistant));
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
