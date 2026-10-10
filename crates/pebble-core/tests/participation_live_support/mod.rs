//! Capture the two explicitly selected Chat Completions services for the opt-in check.
use crate::classifier_support::{Reply, Server};
use pebble_core::{ChatEventKind, ChatSubmission};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
pub type Captures = Arc<Mutex<Vec<Value>>>;

pub fn relay(url: String, kind: &'static str, raw: Captures, clock: Instant) -> Server {
    Server::handling(move |body| {
        let started = clock.elapsed().as_millis();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async {
            let response = reqwest::Client::builder()
                .timeout(Duration::from_secs(90))
                .build()?
                .post(format!("{}/chat/completions", url.trim_end_matches('/')))
                .json(body)
                .send()
                .await?;
            let status = response.status().as_u16();
            Ok::<_, reqwest::Error>((status, response.text().await?))
        });
        let (status, text, error) = match result {
            Ok((status, text)) => (status, text, None),
            Err(error) => (
                502,
                json!({"error":"Selected endpoint transport failed"}).to_string(),
                Some(error.to_string()),
            ),
        };
        let response = serde_json::from_str::<Value>(&text).ok();
        let phase = if kind == "chat" {
            "chat"
        } else {
            let prompt = body["messages"][0]["content"].as_str().unwrap_or("");
            if prompt == include_str!("../../assets/fallback-intent-prompt.txt").trim_end() {
                "intent"
            } else if prompt
                == include_str!("../../assets/fallback-permission-prompt.txt").trim_end()
            {
                "permission"
            } else {
                "primary"
            }
        };
        raw.lock().unwrap().push(json!({"service":kind,"phase":phase,"started_ms":started,"finished_ms":clock.elapsed().as_millis(),"request":body,"http_status":status,"response":response,"response_sse":if kind=="chat"{Some(&text)}else{None},"transport_error":error}));
        let mut reply = if body["stream"] == true && status == 200 {
            Reply::stream(text)
        } else {
            Reply::json(response.unwrap_or_else(|| json!({"raw_invalid_json":text})))
        };
        reply.status = status;
        reply
    })
}
pub fn primary(records: &[Value], persona: &str) -> Option<Value> {
    records
        .iter()
        .filter(|r| r["phase"] == "primary")
        .filter_map(|r| {
            let input: Value =
                serde_json::from_str(r["request"]["messages"][1]["content"].as_str()?).ok()?;
            (input["persona"]["id"] == persona).then_some(r)
        })
        .min_by_key(|r| r["started_ms"].as_u64())
        .cloned()
}
pub fn boolean(record: &Value) -> Option<bool> {
    let value: Value =
        serde_json::from_str(record["response"]["choices"][0]["message"]["content"].as_str()?)
            .ok()?;
    value["respond"].as_bool()
}
pub fn valid_classifier(record: &Value) -> bool {
    let Some(text) = record["response"]["choices"][0]["message"]["content"].as_str() else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let key = if record["phase"] == "intent" {
        "whole_group_question"
    } else {
        "respond"
    };
    record["http_status"] == 200
        && record["transport_error"].is_null()
        && record["response"]["choices"][0]["finish_reason"] == "stop"
        && value.as_object().is_some_and(|fields| {
            fields.len() == 1 && fields.get(key).is_some_and(Value::is_boolean)
        })
}
pub fn nonthinking(record: &Value) -> bool {
    record["response"]["choices"][0]["message"]
        .get("reasoning_content")
        .is_none_or(|v| v.is_null() || v.as_str() == Some(""))
        && record["response"]["usage"]["completion_tokens_details"]
            .get("reasoning_tokens")
            .is_none_or(|v| v == 0)
}
pub fn settle(post: &ChatSubmission, rx: &mpsc::Receiver<pebble_core::ChatEvent>) -> Vec<Value> {
    let mut expected = post.tasks.len();
    let mut done = 0;
    let mut settled = post.group.is_none();
    let mut events = Vec::new();
    while !settled || done < expected {
        let event = rx
            .recv_timeout(Duration::from_secs(180))
            .expect("Bounded live round timed out");
        match event.kind {
            ChatEventKind::Admitted => expected += 1,
            ChatEventKind::GroupSettled => settled = true,
            ChatEventKind::Completed | ChatEventKind::Failed | ChatEventKind::Cancelled => {
                done += 1
            }
            _ => {}
        }
        events.push(json!({"kind":format!("{:?}",event.kind),"run_id":event.run_id,"persona_id":event.persona_id,"text":event.text}));
    }
    events
}
