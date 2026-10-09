mod live_support;
mod responses_support;
use live_support::{Plan, Server, configure, isolated, start, stream, terminal};
use pebble_core::{ChatEventKind, MessageStatus};
use responses_support::{completed, event, items, success};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[test]
fn non_success_or_inconsistent_streams_preserve_partial_text_without_native_output() {
    let partial = event(
        "response.output_text.delta",
        json!({"output_index":1,"content_index":0,"delta":"partial"}),
    );
    let cases = [
        (
            "incomplete",
            format!(
                "{partial}{}",
                event(
                    "response.incomplete",
                    json!({"response":{"status":"incomplete"}})
                )
            ),
        ),
        (
            "failed",
            format!(
                "{partial}{}",
                event(
                    "response.failed",
                    json!({"response":{"status":"failed","error":{"message":"test-secret"}}})
                )
            ),
        ),
        (
            "failed",
            format!(
                "{partial}{}",
                event("error", json!({"message":"test-secret"}))
            ),
        ),
        ("completion marker", partial.clone()),
        ("malformed JSON", format!("{partial}data: {{broken}}\n\n")),
        (
            "differs",
            format!("{partial}{}", completed(&items("different"))),
        ),
        ("without visible", completed(&items("")[..1])),
        (
            "unsupported output item",
            completed(&[
                json!({"id":"call_test","type":"function_call","name":"unsafe_tool","arguments":"{}"}),
            ]),
        ),
        (
            "did not complete",
            completed(&[
                json!({"id":"msg_test","type":"message","role":"assistant","status":"in_progress","content":[]}),
            ]),
        ),
        (
            "did not complete successfully",
            event(
                "response.completed",
                json!({"response":{"status":"incomplete","output":items("partial")}}),
            ),
        ),
    ];
    for (expected, body) in cases {
        let server = Server::new(vec![stream(body)]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "responses", "");
        let (_task, rx) = start(&core, &id, "synthetic failure");
        let events = terminal(&rx);
        let last = events.last().unwrap();
        assert_eq!(last.kind, ChatEventKind::Failed, "{expected}");
        assert!(last.text.contains(expected), "{}", last.text);
        assert!(!last.text.contains("test-secret"));
        assert_eq!(core.messages(&id).unwrap()[1].status, MessageStatus::Failed);
        if events.iter().any(|e| e.kind == ChatEventKind::TextDelta) {
            assert_eq!(core.messages(&id).unwrap()[1].content, "partial");
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
    }
}

#[test]
fn refusal_is_visible_and_native_refusal_is_retained() {
    for streamed in [true, false] {
        let output = vec![
            json!({"id":"refusal_test","type":"message","role":"assistant","status":"completed","content":[{"type":"refusal","refusal":"I cannot help with that.","native_extension":7}]}),
        ];
        let body = format!(
            "{}{}",
            if streamed {
                event(
                    "response.refusal.delta",
                    json!({"output_index":0,"content_index":0,"delta":"I cannot help with that."}),
                )
            } else {
                String::new()
            },
            completed(&output)
        );
        let server = Server::new(vec![stream(body), stream(success("next"))]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "responses", "");
        let (_task, rx) = start(&core, &id, "synthetic refusal prompt");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        assert_eq!(
            core.messages(&id).unwrap()[1].content,
            "I cannot help with that."
        );
        let (task, rx) = start(&core, &id, "next input");
        terminal(&rx);
        let request = core.model_request(&task.id).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&request.items[2].payload).unwrap(),
            output[0]
        );
    }
}

#[test]
fn responses_cancels_before_headers_and_during_idle_stream_without_auth() {
    for headers in [true, false] {
        let server = Server::new(vec![Plan {
            status: 200,
            body: success("partial"),
            delay_headers: headers,
            delay_tail: !headers,
        }]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "responses", "");
        let (task, rx) = start(&core, &id, "cancel");
        let (headers_raw, _) = server
            .requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert!(!headers_raw.to_lowercase().contains("authorization:"));
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
    }
}

#[test]
fn protocol_switch_preserves_transcript_and_starts_native_context() {
    let server = Server::new(vec![
        stream(live_support::success("CHAT_ONLY_OUTPUT")),
        stream(success("responses output")),
        stream(success("responses continuation")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (chat, rx) = start(&core, &id, "CHAT_ONLY_INPUT");
    terminal(&rx);
    let old = core.model_request(&chat.id).unwrap();
    configure(root.path(), &server.url, "mock", "responses", "");
    let (response, rx) = start(&core, &id, "new protocol");
    terminal(&rx);
    let first = core.model_request(&response.id).unwrap();
    assert_eq!(first.items.len(), 2);
    assert_ne!(first.session_id, old.session_id);
    let (next, rx) = start(&core, &id, "continuation");
    terminal(&rx);
    assert!(
        core.model_request(&next.id)
            .unwrap()
            .items
            .iter()
            .all(|i| !String::from_utf8_lossy(&i.payload).contains("CHAT_ONLY"))
    );
    assert_eq!(core.messages(&id).unwrap().len(), 6);
    assert!(
        core.workspace().unwrap().personas[0]
            .connection_label
            .contains("Responses")
    );
}
