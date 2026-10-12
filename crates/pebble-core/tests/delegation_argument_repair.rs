mod delegation_support;
mod live_support;
mod responses_support;
use delegation_support::{configure as codex_configure, method_count, settled};
use live_support::*;
use pebble_core::ChatEventKind;
use serde_json::{Value, json};
use std::time::Duration;

fn native(api: &str, id: &str, arguments: &str) -> Value {
    if api == "responses" {
        json!({"id":format!("item-{id}"),"type":"function_call","status":"completed","call_id":id,"name":"delegate_task","arguments":arguments})
    } else {
        json!({"type":"function","id":id,"function":{"name":"delegate_task","arguments":arguments}})
    }
}
fn tools(api: &str, calls: Vec<Value>, refusal: bool) -> String {
    if api == "responses" {
        let mut output = vec![
            json!({"id":format!("reasoning-{}",calls[0]["id"]),"type":"reasoning","encrypted_content":"sealed-synthetic","summary":[]}),
        ];
        output.extend(calls);
        if refusal {
            output.push(json!({"id":"refusal","type":"message","role":"assistant","status":"completed","content":[{"type":"refusal","refusal":"Synthetic refusal"}]}));
        }
        responses_support::completed(&output)
    } else {
        let calls: Vec<_> = calls
            .into_iter()
            .enumerate()
            .map(|(index, mut call)| {
                call["index"] = json!(index);
                call
            })
            .collect();
        let mut delta = json!({"tool_calls":calls,"reasoning_content":"opaque-synthetic"});
        if refusal {
            delta["refusal"] = json!("Synthetic refusal");
        }
        format!(
            "{}{}data: [DONE]\n\n",
            event(delta, Value::Null),
            event(json!({}), json!("tool_calls"))
        )
    }
}
fn call(api: &str, id: &str, arguments: &str) -> String {
    tools(api, vec![native(api, id, arguments)], false)
}
fn valid() -> String {
    json!({"title":"Synthetic lookup","instructions":"Inspect synthetic evidence only","request_key":"shared","task_id":null}).to_string()
}
fn reply(api: &str, text: &str) -> String {
    if api == "responses" {
        responses_support::success(text)
    } else {
        success(text)
    }
}
fn invalid_reply(api: &str) -> String {
    if api == "responses" {
        responses_support::candidate("not-json")
    } else {
        candidate("not-json")
    }
}
fn input(body: &Value) -> &[Value] {
    body["messages"]
        .as_array()
        .or(body["input"].as_array())
        .unwrap()
}
fn balanced(items: &[Value], expected: usize) {
    let mut pending = None;
    let mut pairs = 0;
    for item in items {
        if let Some(calls) = item["tool_calls"].as_array() {
            assert!(pending.is_none());
            assert_eq!(calls.len(), 1);
            pending = Some(calls[0]["id"].as_str().unwrap().to_owned());
        } else if item["type"] == "function_call" {
            assert!(pending.is_none());
            pending = Some(item["call_id"].as_str().unwrap().to_owned());
        } else if item["role"] == "tool" || item["type"] == "function_call_output" {
            let id = item["tool_call_id"]
                .as_str()
                .or(item["call_id"].as_str())
                .unwrap();
            assert_eq!(pending.take().as_deref(), Some(id));
            pairs += 1;
        }
    }
    assert!(pending.is_none());
    assert_eq!(pairs, expected);
}
fn feedback(body: &Value) -> Value {
    let item = input(body).last().unwrap();
    serde_json::from_str(
        item["content"]
            .as_str()
            .or(item["output"].as_str())
            .unwrap(),
    )
    .unwrap()
}
fn request(server: &Server) -> Value {
    server
        .requests
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .1
}
fn no_effect(core: &pebble_core::Core, root: &std::path::Path, fixture: &std::path::Path) {
    assert!(core.delegated_tasks().unwrap().is_empty());
    assert_eq!(method_count(fixture, "turn/start"), 0);
    let db = rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap();
    let (before, current): (String, String) = db.query_row(
        "SELECT c.state_before,s.current_state FROM model_calls c JOIN persona_states s ON s.persona_id=c.persona_id ORDER BY c.persona_sequence LIMIT 1",
        [], |r| Ok((r.get(0)?,r.get(1)?)),
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&before).unwrap(),
        serde_json::from_str::<Value>(&current).unwrap()
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM delegation_submissions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn corrections_preserve_native_replay_final_candidate_budget_and_accepted_retry_prefix() {
    for api in ["chat_completions", "responses"] {
        for fail_final in [false, true] {
            let missing = json!({"instructions":"Inspect synthetic evidence only","request_key":"synthetic_attempt_2`,task_id:"}).to_string();
            let mut plans = vec![
                stream(call(api, "bad-missing", &missing)),
                stream(call(api, "bad-syntax", "{PRIVATE_BROKEN_JSON")),
                stream(call(api, "accepted", &valid())),
                stream(invalid_reply(api)),
                stream(invalid_reply(api)),
                stream(if fail_final {
                    invalid_reply(api)
                } else {
                    reply(api, "Accepted once")
                }),
            ];
            if fail_final {
                plans.push(stream(reply(api, "Explanation recovered")));
            }
            plans.push(stream(reply(api, "Saved evidence explained")));
            plans.push(stream(reply(api, "Ordinary next reply")));
            let server = Server::memory(plans);
            let (root, core, persona) = isolated();
            configure(root.path(), &server.url, "mock", api, "");
            let fixture = tempfile::tempdir().unwrap();
            codex_configure(&core, fixture.path(), "gated");
            let (_handle, events) = start(&core, &persona, "Inspect synthetic evidence");
            let terminal = terminal(&events).pop().unwrap();
            assert_eq!(
                terminal.kind,
                if fail_final {
                    ChatEventKind::Failed
                } else {
                    ChatEventKind::Completed
                },
                "{}",
                terminal.text
            );
            let bodies: Vec<_> = (0..6).map(|_| request(&server)).collect();
            for index in [1, 2] {
                balanced(input(&bodies[index]), index);
                assert_eq!(bodies[index]["tool_choice"], "auto");
                let error = feedback(&bodies[index]);
                assert_eq!(error["ok"], false);
                assert_eq!(error["corrective_retries_remaining"], 3 - index);
                assert!(
                    error["instruction"]
                        .as_str()
                        .unwrap()
                        .contains("exactly four required fields")
                );
            }
            assert!(
                feedback(&bodies[1])["detail"]
                    .as_str()
                    .unwrap()
                    .contains("title, task_id")
            );
            let syntax = feedback(&bodies[2]);
            assert!(syntax["detail"].as_str().unwrap().contains("syntax error"));
            assert!(!syntax.to_string().contains("PRIVATE"));
            for body in &bodies[3..] {
                balanced(input(body), 3);
                assert_eq!(body["tool_choice"], "none");
            }
            if fail_final {
                let (_retry, events) = retry(&core, &persona, &terminal.message_id);
                assert_eq!(
                    live_support::terminal(&events).last().unwrap().kind,
                    ChatEventKind::Completed
                );
                let body = request(&server);
                balanced(input(&body), 3);
                assert_eq!(body["tool_choice"], "none");
            }
            std::fs::write(fixture.path().join("release"), "synthetic").unwrap();
            let task = settled(&core);
            assert_eq!(task.report_state, "completed");
            request(&server);
            assert_eq!(method_count(fixture.path(), "turn/start"), 1);
            assert_eq!(core.delegated_tasks().unwrap().len(), 1);
            drop(_handle);
            drop(core);
            let core = reopen(root.path());
            let (_next, events) = start(&core, &persona, "Continue ordinary chat");
            assert_eq!(
                live_support::terminal(&events).last().unwrap().kind,
                ChatEventKind::Completed
            );
            let body = request(&server);
            balanced(input(&body), 3);
            assert!(body.to_string().contains(if api == "responses" {
                "sealed-synthetic"
            } else {
                "opaque-synthetic"
            }));
            assert_eq!(method_count(fixture.path(), "turn/start"), 1);
        }
    }
}

#[test]
fn three_invalid_attempts_exhaust_without_effects_and_explicit_retry_can_correct() {
    for api in ["chat_completions", "responses"] {
        let mut absent: Value = serde_json::from_str(&valid()).unwrap();
        absent.as_object_mut().unwrap().remove("task_id");
        let server = Server::memory(vec![
            stream(call(api, "bad-1", &absent.to_string())),
            stream(call(api, "bad-2", &absent.to_string())),
            stream(call(api, "bad-3", &absent.to_string())),
            stream(call(api, "valid-retry", &valid())),
            stream(reply(api, "Accepted")),
            stream(reply(api, "Evidence")),
        ]);
        let (root, core, persona) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let fixture = tempfile::tempdir().unwrap();
        codex_configure(&core, fixture.path(), "complete");
        let (_handle, events) = start(&core, &persona, "Synthetic lookup");
        let terminal = terminal(&events).pop().unwrap();
        assert_eq!(terminal.kind, ChatEventKind::Failed);
        assert!(
            terminal
                .text
                .contains("after 3 attempts: Missing required fields: task_id")
        );
        no_effect(&core, root.path(), fixture.path());
        for index in 0..3 {
            let body = request(&server);
            balanced(input(&body), index);
        }
        let frozen = core.model_request(&terminal.run_id).unwrap();
        let items: Vec<Value> = frozen
            .items
            .iter()
            .map(|item| serde_json::from_slice(&item.payload).unwrap())
            .collect();
        balanced(&items, 3);
        assert_eq!(
            serde_json::from_str::<Value>(
                items.last().unwrap()[if api == "responses" {
                    "output"
                } else {
                    "content"
                }]
                .as_str()
                .unwrap()
            )
            .unwrap()["corrective_retries_remaining"],
            0
        );
        let (_retry, events) = retry(&core, &persona, &terminal.message_id);
        assert_eq!(
            live_support::terminal(&events).last().unwrap().kind,
            ChatEventKind::Completed
        );
        balanced(input(&request(&server)), 3);
        settled(&core);
        assert_eq!(method_count(fixture.path(), "turn/start"), 1);
    }
}

#[test]
fn cancellation_during_correction_preserves_a_balanced_prefix_without_dispatch() {
    for api in ["chat_completions", "responses"] {
        let mut delayed = stream(call(api, "corrected", &valid()));
        delayed.delay_headers = true;
        let server = Server::memory(vec![
            stream(call(api, "invalid", "{}")),
            delayed,
            stream(reply(api, "Later conversation")),
        ]);
        let (root, core, persona) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let fixture = tempfile::tempdir().unwrap();
        codex_configure(&core, fixture.path(), "complete");
        let (handle, events) = start(&core, &persona, "Synthetic lookup");
        request(&server);
        balanced(input(&request(&server)), 1);
        handle.cancel();
        assert_eq!(
            terminal(&events).last().unwrap().kind,
            ChatEventKind::Cancelled
        );
        no_effect(&core, root.path(), fixture.path());
        let (_later, events) = start(&core, &persona, "Ordinary chat after cancellation");
        assert_eq!(
            terminal(&events).last().unwrap().kind,
            ChatEventKind::Completed
        );
        balanced(input(&request(&server)), 1);
    }
}

#[test]
fn ambiguous_multiple_unsupported_and_refused_calls_remain_hard_failures() {
    for api in ["chat_completions", "responses"] {
        for mode in ["identity", "multiple", "unsupported", "refusal"] {
            let mut calls = vec![native(api, "synthetic", &valid())];
            match mode {
                "identity" => {
                    calls[0][if api == "responses" { "call_id" } else { "id" }] = Value::Null;
                }
                "multiple" => calls.push(native(api, "second", "{}")),
                "unsupported" => {
                    if api == "responses" {
                        calls[0]["name"] = json!("unsupported");
                    } else {
                        calls[0]["function"]["name"] = json!("unsupported");
                    }
                }
                _ => {}
            }
            let server = Server::memory(vec![stream(tools(api, calls, mode == "refusal"))]);
            let (root, core, persona) = isolated();
            configure(root.path(), &server.url, "mock", api, "");
            let fixture = tempfile::tempdir().unwrap();
            codex_configure(&core, fixture.path(), "complete");
            let (_handle, events) = start(&core, &persona, "Synthetic lookup");
            assert_eq!(
                terminal(&events).last().unwrap().kind,
                ChatEventKind::Failed,
                "{api}: {mode}"
            );
            request(&server);
            no_effect(&core, root.path(), fixture.path());
            assert!(server.requests.try_recv().is_err());
        }
    }
}
