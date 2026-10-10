mod classifier_support;
mod group_support;
mod live_support;
use classifier_support::{Reply, Server, answer, configure, finish, submit};
use group_support::{count, db};
use pebble_core::ChatEventKind;
use serde_json::{Value, json};

fn classify(api: &str, body: Value, accepted: bool) {
    let (root, core, persona) = live_support::isolated();
    let server = Server::new(vec![Reply::json(body)]);
    configure(&core, None, &server.url, api, "");
    let room = core
        .create_group("Classifier output contract", std::slice::from_ref(&persona))
        .unwrap();
    let (submission, rx) = submit(&core, &room.id, "Synthetic participation input", &[]);
    let events = finish(&submission, &rx);
    let decision = core.participation(&room.id).unwrap().remove(0);
    assert_eq!(
        decision.status,
        if accepted { "selected" } else { "failed" }
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == ChatEventKind::Admitted)
            .count(),
        usize::from(accepted)
    );
    assert_eq!(
        count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
        i64::from(accepted)
    );
    assert_eq!(
        core.room_messages(&room.id).unwrap().len(),
        if accepted { 2 } else { 1 }
    );
    if !accepted {
        assert!(
            decision.respond.is_none() && decision.call_id.is_none() && !decision.error.is_empty()
        );
        assert!(events.iter().any(|event| {
            event.kind == ChatEventKind::Participation
                && event
                    .participation
                    .iter()
                    .any(|d| d.status == "failed" && !d.error.is_empty())
        }));
    }
}

fn with_parts(api: &str, parts: Value) -> Value {
    let mut body = answer(api, "{\"respond\":true}");
    if api == "responses" {
        body["output"][0]["content"] = parts;
    } else {
        body["choices"][0]["message"]["content"] = parts;
    }
    body
}

#[test]
fn valid_boolean_with_unknown_content_is_failed_without_admission() {
    for api in ["chat_completions", "responses"] {
        classify(
            api,
            with_parts(
                api,
                json!([
                    {"type":"output_text","text":"{\"respond\":true}"},
                    {"type":"unknown_synthetic_part","text":"unsupported"}
                ]),
            ),
            false,
        );
    }
}

#[test]
fn valid_boolean_with_tool_content_is_failed_without_admission() {
    for api in ["chat_completions", "responses"] {
        classify(
            api,
            with_parts(
                api,
                json!([
                    {"type":"output_text","text":"{\"respond\":true}"},
                    {"type":"tool_call","name":"unsupported","arguments":"{}"}
                ]),
            ),
            false,
        );
    }
}

#[test]
fn valid_boolean_with_function_call_is_failed_without_admission() {
    for api in ["chat_completions", "responses"] {
        let mut body = answer(api, "{\"respond\":true}");
        if api == "responses" {
            body["output"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"function_call","name":"unsupported","arguments":"{}"}));
        } else {
            body["choices"][0]["message"]["function_call"] =
                json!({"name":"unsupported","arguments":"{}"});
        }
        classify(api, body, false);
    }
}

#[test]
fn normal_text_and_supported_parts_still_admit_while_refusal_remains_terminal() {
    for api in ["chat_completions", "responses"] {
        let mut body = answer(api, "{\"respond\":true}");
        if api == "chat_completions" {
            body["choices"][0]["message"]["function_call"] = Value::Null;
        }
        classify(api, body, true);
        classify(
            api,
            with_parts(
                api,
                json!([
                    {"type":"output_text","text":"{\"respond\":"},
                    {"type":"output_text","text":"true}"}
                ]),
            ),
            true,
        );
        classify(
            api,
            with_parts(
                api,
                json!([
                    {"type":"output_text","text":"{\"respond\":true}"},
                    {"type":"refusal","refusal":"Synthetic refusal"}
                ]),
            ),
            false,
        );
    }
}
