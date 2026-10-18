//! Opt-in live persona validation with synthetic evidence and a gated task executor.
mod delegation_support;
mod live_support;
use live_support::*;
use pebble_core::{ChatEventKind, CredentialChange, DelegationState, SettingsUpdate};
use serde_json::json;
use std::time::{Duration, Instant};

fn eventually(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(180);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "Live persona activity did not settle"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "requires explicitly selected live persona endpoint; synthetic data and local fake executor only"]
fn current_endpoint_explores_accepts_chat_and_assimilates_saved_evidence() {
    let url = std::env::var("PEBBLE_LIVE_API_ROOT").expect("set PEBBLE_LIVE_API_ROOT");
    let model = std::env::var("PEBBLE_LIVE_MODEL").expect("set PEBBLE_LIVE_MODEL");
    let api = std::env::var("PEBBLE_LIVE_API").unwrap_or_else(|_| "chat_completions".into());
    let key = std::env::var("PEBBLE_LIVE_API_KEY").unwrap_or_default();
    let (root, core, id) = isolated();
    configure(root.path(), &url, &model, &api, &key);
    let mut settings = core.settings(None).unwrap();
    settings.chat.context_len = Some(
        std::env::var("PEBBLE_LIVE_CONTEXT_LEN")
            .unwrap_or_else(|_| "262144".into())
            .parse()
            .unwrap(),
    );
    settings.chat.max_output_tokens = Some(
        std::env::var("PEBBLE_LIVE_MAX_OUTPUT_TOKENS")
            .unwrap_or_else(|_| "4096".into())
            .parse()
            .unwrap(),
    );
    settings.chat.request_json =
        std::env::var("PEBBLE_LIVE_REQUEST_JSON").unwrap_or_else(|_| "{}".into());
    core.save_settings(
        None,
        &SettingsUpdate {
            chat: settings.chat,
            credential: CredentialChange::Keep,
            persona: None,
        },
    )
    .unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    std::fs::write(std::path::Path::new(&persona.directory).join("SOUL.md"),
        "You are an isolated synthetic integration-test persona. In your first private_activity, use delegate_task exactly once to investigate a synthetic blue-bird fact, with title Synthetic curiosity and request_key synthetic-curiosity. The executor is a local fixture; do not request real files or external actions. After acceptance wait quietly for its outcome, never dispatch another task. When task_outcomes arrive, acknowledge their exact IDs and revisions and summarize their evidence with any limitations. Always use plans:null and share_intent_id:null in private activity. Never publish an unsolicited chat. For ordinary user messages, answer their request normally and use activity:null; do not dispatch tools unless explicitly asked. Preserve current state.").unwrap();
    let executor = tempfile::tempdir().unwrap();
    delegation_support::configure(&core, executor.path(), "gated");
    std::fs::write(executor.path().join("fake-options.json"),json!({"mode":"gated","artifact":false,"text":"Synthetic fixture evidence only: the blue bird has two wings. This is not a real web finding."}).to_string()).unwrap();
    let mut policy = core.activity_settings(&id).unwrap();
    policy.enabled = true;
    policy.daily_wake_budget = 2;
    policy.quiet_start_minutes = None;
    policy.quiet_end_minutes = None;
    core.save_activity_settings(&id, &policy).unwrap();
    eventually(|| {
        core.delegated_tasks()
            .unwrap()
            .first()
            .is_some_and(|t| t.state == DelegationState::Running)
    });
    println!("Live persona delegated one private task through the actual tool stream.");
    let (chat, events) = start(
        &core,
        &id,
        "Answer me now while your private task is still running. Reply with exactly LIVE_CHAT_OK.",
    );
    let events = terminal(&events);
    assert_eq!(
        events.last().unwrap().kind,
        ChatEventKind::Completed,
        "{}",
        events.last().unwrap().text
    );
    assert!(
        core.messages(&id)
            .unwrap()
            .last()
            .unwrap()
            .content
            .contains("LIVE_CHAT_OK")
    );
    assert_eq!(
        core.delegated_tasks().unwrap()[0].state,
        DelegationState::Running
    );
    drop(chat);
    println!("Live chat completed while the accepted private task continued.");
    std::fs::write(executor.path().join("release"), "").unwrap();
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    eventually(|| {
        let tasks = core.delegated_tasks().unwrap();
        let failure: Option<String> = db.query_row("SELECT error FROM activity_calls WHERE status='failed' ORDER BY rowid DESC LIMIT 1",[],|r|r.get(0)).unwrap_or(None);
        assert!(failure.is_none(), "Private acceptance failed: {failure:?}");
        tasks[0].report_state == "private_assimilated"
    });
    let tasks = core.delegated_tasks().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].state, DelegationState::Completed);
    assert_eq!(
        delegation_support::method_count(executor.path(), "turn/start"),
        1
    );
    assert_eq!(core.messages(&id).unwrap().len(), 2);
    assert_eq!(
        db.query_row(
            "SELECT wakes_used FROM persona_activity_runtime WHERE persona_id=?1",
            [&id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    let summary = core.persona_activity(&id).unwrap().summary;
    assert!(!summary.trim().is_empty());
    println!(
        "Live handover verified: private_assimilated, two wakes, one task, no private chat bubbles. Summary: {summary}"
    );
    core.stop_activity(&id).unwrap();
}
