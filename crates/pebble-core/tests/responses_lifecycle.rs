mod live_support;
mod responses_support;
use live_support::{Server, configure, isolated, start, stream, terminal};
use pebble_core::ChatEventKind;
use responses_support::{completed, event, items, success};
use serde_json::{Value, json};

#[test]
fn id_only_events_use_content_part_lifecycle_and_authoritative_item_order() {
    let mut output = items("alpha");
    output[1]["content"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"output_text","text":"β 🪨","annotations":[]}));
    let mut body = event(
        "response.output_item.added",
        json!({"item":{"id":"msg_test","type":"message","role":"assistant","status":"in_progress","content":[]}}),
    );
    for text in ["alpha", "β 🪨"] {
        body += &event(
            "response.content_part.added",
            json!({"item_id":"msg_test","part":{"type":"output_text","text":""}}),
        );
        body += &event(
            "response.output_text.delta",
            json!({"item_id":"msg_test","delta":text}),
        );
        body += &event(
            "response.output_text.done",
            json!({"item_id":"msg_test","text":text}),
        );
        body += &event(
            "response.content_part.done",
            json!({"item_id":"msg_test","part":{"type":"output_text","text":text}}),
        );
    }
    body += &event("response.output_item.done", json!({"item":output[1]}));
    body += &event("response.output_item.done", json!({"item":output[0]}));
    body += &completed(&output);
    let server = Server::new(vec![stream(body), stream(success("next"))]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "responses", "");
    let (_task, rx) = start(&core, &id, "ID-only lifecycle");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    assert_eq!(core.messages(&id).unwrap()[1].content, "alphaβ 🪨");
    let (task, rx) = start(&core, &id, "next");
    terminal(&rx);
    let request = core.model_request(&task.id).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&request.items[2].payload).unwrap(),
        output[0]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&request.items[3].payload).unwrap(),
        output[1]
    );
}

#[test]
fn missing_or_ambiguous_content_lifecycle_fails_without_guessing_an_index() {
    let cases = [
        (
            "no active content part",
            event(
                "response.output_text.delta",
                json!({"item_id":"msg_test","delta":"ambiguous"}),
            ),
        ),
        (
            "overlapping content parts",
            format!(
                "{}{}",
                event(
                    "response.content_part.added",
                    json!({"item_id":"msg_test","part":{"type":"output_text"}})
                ),
                event(
                    "response.content_part.added",
                    json!({"item_id":"msg_test","part":{"type":"output_text"}})
                )
            ),
        ),
        (
            "differs",
            format!(
                "{}{}",
                event(
                    "response.output_text.delta",
                    json!({"output_index":1,"content_index":0,"delta":"partial"})
                ),
                event(
                    "response.output_text.done",
                    json!({"output_index":1,"content_index":0,"text":"different"})
                )
            ),
        ),
    ];
    for (expected, body) in cases {
        let server = Server::new(vec![stream(body)]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", "responses", "");
        let (_task, rx) = start(&core, &id, "malformed lifecycle");
        let events = terminal(&rx);
        assert_eq!(events.last().unwrap().kind, ChatEventKind::Failed);
        assert!(
            events.last().unwrap().text.contains(expected),
            "{}",
            events.last().unwrap().text
        );
    }
}
