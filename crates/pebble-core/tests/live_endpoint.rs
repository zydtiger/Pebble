mod live_support;
use live_support::*;
use pebble_core::{ChatEventKind, CredentialChange, SettingsUpdate};
use serde_json::Value;
use std::time::Duration;

#[test]
#[ignore = "requires an explicitly selected live endpoint; synthetic isolated data only"]
fn opt_in_live_two_turns_restart_and_cancel() {
    exercise("chat_completions");
}

#[test]
#[ignore = "requires an explicitly selected Responses endpoint; synthetic isolated data only"]
fn opt_in_live_responses_two_turns_restart_and_cancel() {
    exercise("responses");
}

fn exercise(api: &str) {
    let url = std::env::var("PEBBLE_LIVE_API_ROOT").expect("set PEBBLE_LIVE_API_ROOT");
    let model = std::env::var("PEBBLE_LIVE_MODEL").expect("set PEBBLE_LIVE_MODEL");
    let key = std::env::var("PEBBLE_LIVE_API_KEY").unwrap_or_default();
    let (root, core, id) = isolated();
    configure(root.path(), &url, &model, api, &key);
    if api == "responses"
        && let Ok(format) = std::env::var("PEBBLE_LIVE_RESPONSES_SCHEMA_FORMAT")
    {
        use std::io::Write;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(root.path().join("config.toml"))
                .unwrap(),
            "responses_schema_format = {format:?}"
        )
        .unwrap();
    }
    let settings = core.settings(None).unwrap();
    core.save_settings(
        None,
        &SettingsUpdate {
            chat: settings.chat,
            credential: CredentialChange::Keep,
            context_window_tokens: Some(65536),
            max_output_tokens: Some(1024),
            persona: None,
        },
    )
    .unwrap();
    let (task, rx) = start(&core, &id, "Reply with exactly: PEBBLE_OK");
    let events = terminal(&rx);
    assert_eq!(
        events.last().unwrap().kind,
        ChatEventKind::Completed,
        "{}",
        events.last().unwrap().text
    );
    let first = core.model_request(&task.id).unwrap();
    assert!(first.response_schema.is_some());
    assert!(events.iter().all(|e| e.kind != ChatEventKind::TextDelta));
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let saved_state: String = db
        .query_row(
            "SELECT current_state FROM persona_states WHERE persona_id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    let saved = core.messages(&id).unwrap();
    assert!(saved[1].content.contains("PEBBLE_OK"));
    println!(
        "Live turn 1: completed, visible_content={:?}, deltas={}",
        saved[1].content,
        events
            .iter()
            .filter(|e| e.kind == ChatEventKind::TextDelta)
            .count()
    );
    drop(task);
    drop(core);
    let core = reopen(root.path());
    assert_eq!(saved, core.messages(&id).unwrap());
    assert_eq!(
        db.query_row(
            "SELECT current_state FROM persona_states WHERE persona_id=?1",
            [&id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        saved_state
    );
    let (task, rx) = start(&core, &id, "Reply with exactly: PEBBLE_REPLAY_OK");
    let events = terminal(&rx);
    assert_eq!(
        events.last().unwrap().kind,
        ChatEventKind::Completed,
        "{}",
        events.last().unwrap().text
    );
    let request = core.model_request(&task.id).unwrap();
    assert_eq!(request.session_id, first.session_id);
    assert_eq!(
        request.state_before,
        Some(serde_json::from_str::<Value>(&saved_state).unwrap())
    );
    assert!(request.items.len() >= 4);
    assert_eq!(request.protocol, api);
    for (a, b) in first.items.iter().zip(&request.items) {
        assert_eq!(a.payload, b.payload);
    }
    let output: Vec<Value> = request.items[2..request.items.len() - 1]
        .iter()
        .map(|item| serde_json::from_slice(&item.payload).unwrap())
        .collect();
    println!(
        "Live turn 2: completed, visible_content={:?}, replay_items={}, retained_reasoning={}",
        core.messages(&id).unwrap()[3].content,
        request.items.len(),
        output
            .iter()
            .any(|item| item.get("reasoning_content").is_some() || item["type"] == "reasoning")
    );
    let (cancel, rx) = start(&core, &id, "Reply with exactly: PEBBLE_RETRY_OK");
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    cancel.cancel();
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Cancelled);
    println!("Live cancellation: cancelled before first visible token; temporary root isolated");
    let frozen = core.model_request(&cancel.id).unwrap();
    let reply = cancel.messages.last().unwrap().id.clone();
    let (retried, rx) = retry(&core, &id, &reply);
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    let request = core.model_request(&retried.id).unwrap();
    assert_eq!(request.session_id, frozen.session_id);
    for (a, b) in request.items.iter().zip(&frozen.items) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.payload, b.payload);
    }
    let messages = core.messages(&id).unwrap();
    assert_eq!(
        messages.iter().filter(|m| m.sender_kind == "user").count(),
        3
    );
    assert!(messages.last().unwrap().content.contains("PEBBLE_RETRY_OK"));
    println!(
        "Live explicit retry: completed, visible_content={:?}, original_user_count=3, frozen_request_unchanged=true",
        messages.last().unwrap().content
    );
}
