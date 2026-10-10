// Native synthetic Responses fixtures, including opaque state that must survive replay.
#![allow(dead_code)]
use serde_json::{Value, json};

pub(super) fn event(kind: &str, mut body: Value) -> String {
    body["type"] = kind.into();
    format!("event: {kind}\r\ndata: {body}\r\n\r\n")
}
pub(super) fn structured(text: &str) -> String {
    json!({"reply":text,"state":{"affinity_user":20,"mood":"calm"}}).to_string()
}
pub(super) fn items(text: &str) -> Vec<Value> {
    raw_items(&structured(text))
}
pub(super) fn raw_items(text: &str) -> Vec<Value> {
    vec![
        json!({"id":"rs_test", "type":"reasoning", "summary":[], "content":[{"type":"reasoning_text","text":"hidden synthetic reasoning"}], "encrypted_content":"sealed-test-state", "unknown_reasoning_field":{"opaque":[1,"retained",null]}}),
        json!({"id":"msg_test", "type":"message", "role":"assistant", "status":"completed", "content":[{"type":"output_text","text":text,"annotations":[],"unknown_text_field":"keep"}], "unknown_message_field":{"nested":true}}),
    ]
}
pub(super) fn completed(items: &[Value]) -> String {
    event(
        "response.completed",
        json!({"response":{"id":"resp_test","status":"completed","output":items}}),
    )
}
pub(super) fn success(text: &str) -> String {
    candidate(&structured(text))
}
pub(super) fn candidate(text: &str) -> String {
    let final_items = raw_items(text);
    format!(
        "{}{}{}{}{}{}",
        event(
            "response.output_item.added",
            json!({"output_index":0,"item":{"type":"reasoning","id":"rs_test","summary":[]}})
        ),
        event(
            "response.reasoning_text.delta",
            json!({"output_index":0,"delta":"hidden synthetic reasoning"})
        ),
        event(
            "response.output_text.delta",
            json!({"output_index":1,"content_index":0,"item_id":"msg_test","delta":text})
        ),
        // Done events may arrive in a different order; native output order comes from completed.output.
        event(
            "response.output_item.done",
            json!({"output_index":1,"item":final_items[1]})
        ),
        event(
            "response.output_item.done",
            json!({"output_index":0,"item":final_items[0]})
        ),
        completed(&final_items)
    )
}
