mod live_support;
use live_support::*;
use pebble_core::ChatEventKind;
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
    let config_path = root.path().join("config.toml");
    let connection = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(&config_path, format!("{connection}\n[defaults.context]\ncontext_window_tokens=65536\nmax_output_tokens=1024\n")).unwrap();
    let (task, rx) = start(&core, &id, "Reply with exactly: PEBBLE_OK");
    let events = terminal(&rx);
    assert_eq!(
        events.last().unwrap().kind,
        ChatEventKind::Completed,
        "{}",
        events.last().unwrap().text
    );
    let first = core.model_request(&task.id).unwrap();
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
    let (cancel, rx) = start(&core, &id, "Write a long synthetic story about a stone.");
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    cancel.cancel();
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Cancelled);
    println!("Live cancellation: cancelled before first visible token; temporary root isolated");
}
