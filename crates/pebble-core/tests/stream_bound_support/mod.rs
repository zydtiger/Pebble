//! Shared streaming fixtures for decoded-output bounds and saved task reports.
#![allow(dead_code)]
use serde_json::{Value, json};

pub fn response_event(kind: &str, mut body: Value) -> String {
    body["type"] = kind.into();
    format!("event: {kind}\ndata: {body}\n\n")
}

/// Small real output with enough per-token protocol metadata to exceed 128 KiB.
pub fn verbose_success(protocol: &str, reply: &str) -> String {
    let text = super::live_support::structured(reply);
    let mut body = String::new();
    for (sequence, token) in text.chars().enumerate() {
        if protocol == "responses" {
            body.push_str(&response_event(
                "response.output_text.delta",
                json!({
                    "output_index":0,"content_index":0,"item_id":"msg_verbose",
                    "sequence_number":sequence,"delta":token.to_string(),
                    "obfuscation":"sample-padding"
                }),
            ));
        } else {
            body.push_str(&format!("data: {}\n\n", json!({
                "id":"chatcmpl-verbose-fixture","object":"chat.completion.chunk",
                "created":1_792_000_000,"model":"mock-streaming-model",
                "choices":[{"index":0,"delta":{"content":token.to_string()},"finish_reason":null}]
            })));
        }
    }
    if protocol == "responses" {
        body.push_str(&response_event(
            "response.output_text.done",
            json!({
                "output_index":0,"content_index":0,"item_id":"msg_verbose","text":text
            }),
        ));
        let item = json!({"id":"msg_verbose","type":"message","role":"assistant",
            "status":"completed","content":[{"type":"output_text","text":text}]});
        body.push_str(&response_event(
            "response.output_item.done",
            json!({"output_index":0,"item":item}),
        ));
        body.push_str(&response_event(
            "response.completed",
            json!({"response":{"status":"completed","output":[item]}}),
        ));
    } else {
        body.push_str(&super::live_support::event(json!({}), json!("stop")));
        body.push_str("data: [DONE]\n\n");
    }
    assert!(body.len() > 128 * 1024);
    body
}
