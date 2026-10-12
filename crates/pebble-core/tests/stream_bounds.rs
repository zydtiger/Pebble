mod live_support;
mod responses_support;
mod stream_bound_support;
use live_support::*;
use pebble_core::ChatEventKind;
use serde_json::{Value, json};
use stream_bound_support::{response_event, verbose_success};

#[test]
fn verbose_wire_metadata_does_not_spend_decoded_output_for_either_protocol() {
    let reply = "x".repeat(900);
    for protocol in ["chat_completions", "responses"] {
        let server = Server::memory(vec![stream(verbose_success(protocol, &reply))]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", protocol, "");
        let (task, rx) = start(&core, &id, "Synthetic metadata-heavy stream.");
        let events = terminal(&rx);
        assert_eq!(
            events.last().unwrap().kind,
            ChatEventKind::Completed,
            "{events:?}"
        );
        assert_eq!(core.messages(&id).unwrap().last().unwrap().content, reply);
        assert!(core.model_request(&task.id).unwrap().memory_capable);
    }
}

#[test]
fn responses_repeated_text_done_items_and_final_preserve_exact_native_output() {
    let reply = "x".repeat(80_000);
    let text = structured(&reply);
    let items = responses_support::raw_items(&text);
    let body = format!(
        "{}{}{}{}{}",
        response_event(
            "response.output_text.delta",
            json!({"output_index":1,"content_index":0,"delta":text})
        ),
        response_event(
            "response.output_text.done",
            json!({"output_index":1,"content_index":0,"text":text})
        ),
        response_event(
            "response.output_item.done",
            json!({"output_index":1,"item":items[1]})
        ),
        response_event(
            "response.output_item.done",
            json!({"output_index":0,"item":items[0]})
        ),
        responses_support::completed(&items)
    );
    assert!(body.len() > 3 * 80_000);
    let server = Server::memory(vec![
        stream(body),
        stream(responses_support::success("next")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "responses", "");
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("context_len=65536", "context_len=262144"),
    )
    .unwrap();
    let (_task, rx) = start(&core, &id, "Synthetic repeated representations.");
    let events = terminal(&rx);
    assert_eq!(
        events.last().unwrap().kind,
        ChatEventKind::Completed,
        "{events:?}"
    );
    assert_eq!(core.messages(&id).unwrap().last().unwrap().content, reply);
    let (_task, rx) = start(&core, &id, "Check native replay.");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    server.requests.recv().unwrap();
    let replay = server.requests.recv().unwrap().1;
    for item in items {
        assert!(replay["input"].as_array().unwrap().contains(&item));
    }
}

#[test]
fn native_tool_argument_limit_remains_independent_of_decoded_output_limit() {
    let arguments = "x".repeat(8192 * 6 + 15);
    let cases = [
        (
            "chat_completions",
            event(
                json!({"tool_calls":[{"index":0,"id":"call","type":"function","function":{"name":"update_memory","arguments":arguments}}]}),
                Value::Null,
            ),
        ),
        (
            "responses",
            response_event(
                "response.output_item.done",
                json!({"output_index":0,"item":{"id":"fc","type":"function_call","call_id":"call","name":"update_memory","arguments":arguments}}),
            ),
        ),
        (
            "responses",
            response_event(
                "response.function_call_arguments.delta",
                json!({"output_index":0,"delta":arguments}),
            ),
        ),
    ];
    for (protocol, body) in cases {
        let server = Server::memory(vec![stream(body)]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", protocol, "");
        let (_task, rx) = start(&core, &id, "Synthetic oversized tool arguments.");
        let events = terminal(&rx);
        let last = events.last().unwrap();
        assert_eq!(last.kind, ChatEventKind::Failed);
        assert!(
            last.text.contains("Native tool arguments exceeded"),
            "{}",
            last.text
        );
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn responses_index_only_text_keeps_its_identity_when_an_item_id_arrives() {
    let text = structured("hello");
    let (first, second) = text.split_at(15);
    let items = responses_support::raw_items(&text);
    let body = format!(
        "{}{}{}{}",
        response_event(
            "response.output_text.delta",
            json!({"output_index":1,"content_index":0,"delta":first})
        ),
        response_event(
            "response.output_item.added",
            json!({"output_index":1,"item":{"id":"msg_test","type":"message","content":[]}})
        ),
        response_event(
            "response.output_text.delta",
            json!({"output_index":1,"content_index":0,"delta":second})
        ),
        responses_support::completed(&items)
    );
    let server = Server::memory(vec![stream(body)]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "responses", "");
    let (_task, rx) = start(&core, &id, "Synthetic late ID.");
    let events = terminal(&rx);
    assert_eq!(
        events.last().unwrap().kind,
        ChatEventKind::Completed,
        "{events:?}"
    );
}

#[test]
fn decoded_reply_reasoning_and_opaque_output_fail_before_acceptance() {
    let oversized = "x".repeat(128 * 1024 + 1);
    let mut opaque = responses_support::items("small");
    opaque[0]["encrypted_content"] = oversized.clone().into();
    let cases = [
        (
            "chat_completions",
            event(json!({"content":oversized}), Value::Null),
        ),
        (
            "chat_completions",
            event(json!({"reasoning_content":oversized}), Value::Null),
        ),
        (
            "responses",
            response_event(
                "response.reasoning_text.delta",
                json!({"output_index":0,"delta":oversized}),
            ),
        ),
        (
            "responses",
            response_event(
                "response.output_item.done",
                json!({"output_index":0,"item":opaque[0]}),
            ),
        ),
        ("responses", responses_support::completed(&opaque)),
    ];
    for (protocol, body) in cases {
        let server = Server::memory(vec![stream(body)]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", protocol, "");
        let (_task, rx) = start(&core, &id, "Synthetic oversized output.");
        let events = terminal(&rx);
        let last = events.last().unwrap();
        assert_eq!(last.kind, ChatEventKind::Failed);
        assert!(
            last.text.contains("128 KiB decoded output limit"),
            "{}",
            last.text
        );
        assert!(
            core.messages(&id)
                .unwrap()
                .last()
                .unwrap()
                .content
                .is_empty()
        );
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
            db.query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
