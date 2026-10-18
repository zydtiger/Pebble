//! Opt-in: real persona endpoint, scheduler, Codex web search, and private handover.
mod live_support;
use live_support::*;
use pebble_core::{
    ChatEventKind, CodexSettings, CredentialChange, DelegatedTask, DelegationListener,
    DelegationState, SettingsUpdate,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
struct Observer(Mutex<(bool, String)>);
impl DelegationListener for Observer {
    fn on_tasks(&self, tasks: Vec<DelegatedTask>) -> Result<(), String> {
        let mut observed = self.0.lock().unwrap();
        for task in tasks {
            observed.0 |= task.activity == "Searching the web…";
            let progress = format!(
                "{:?}: {} / {}",
                task.state, task.activity, task.report_state
            );
            if observed.1 != progress {
                println!("Codex {progress}");
                observed.1 = progress;
            }
        }
        Ok(())
    }
}

#[test]
#[ignore = "requires explicit live persona configuration and authenticated Codex; performs public web research"]
fn real_heartbeat_searches_chats_and_assimilates() {
    let (root, core, id) = isolated();
    configure(
        root.path(),
        &std::env::var("PEBBLE_LIVE_API_ROOT").expect("set PEBBLE_LIVE_API_ROOT"),
        &std::env::var("PEBBLE_LIVE_MODEL").expect("set PEBBLE_LIVE_MODEL"),
        &std::env::var("PEBBLE_LIVE_API").unwrap_or_else(|_| "chat_completions".into()),
        &std::env::var("PEBBLE_LIVE_API_KEY").unwrap_or_default(),
    );
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
        "You are an isolated integration-test persona curious about sourdough. On the first private_activity, use delegate_task exactly once to research how lactic acid bacteria contribute to sourdough fermentation. Ask Codex to use live web search, find one reliable public source, and return two factual sentences with a precise source URL. Explicitly tell it not to access local files or personal accounts, write files, send messages, or delegate recursively. Use request_key sourdough-curiosity. After acceptance wait quietly; do not dispatch another task. When task_outcomes arrive, acknowledge their exact IDs/revisions and summarize the findings and any limitations. Preserve state, use plans:null and share_intent_id:null for private activity, and never publish an unsolicited chat. For ordinary chat, answer the user's request directly with activity:null and no tools.").unwrap();
    core.save_codex_settings(&CodexSettings {
        model: std::env::var("PEBBLE_LIVE_CODEX_MODEL").ok(),
        effort: std::env::var("PEBBLE_LIVE_CODEX_EFFORT").ok(),
        ..CodexSettings::default()
    })
    .unwrap();
    let observer = Arc::new(Observer::default());
    core.observe_delegations(observer.clone()).unwrap();
    let mut policy = core.activity_settings(&id).unwrap();
    policy.enabled = true;
    policy.daily_wake_budget = 2;
    policy.quiet_start_minutes = None;
    policy.quiet_end_minutes = None;
    let start_time = Instant::now();
    core.save_activity_settings(&id, &policy).unwrap();
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let mut chatted = false;
    loop {
        let failure: Option<String> = db.query_row(
            "SELECT error FROM activity_calls WHERE status='failed' ORDER BY rowid DESC LIMIT 1",
            [], |r| r.get(0),
        ).unwrap_or(None);
        assert!(failure.is_none(), "Private heartbeat failed: {failure:?}");
        let tasks = core.delegated_tasks().unwrap();
        if let Some(task) = tasks.first() {
            assert_eq!(tasks.len(), 1, "Exploration dispatched more than once");
            if !chatted && task.state == DelegationState::Running {
                assert!(core.messages(&id).unwrap().is_empty());
                let (_chat, events) = start(
                    &core,
                    &id,
                    "Please reply with exactly LIVE_CHAT_OK while your research continues.",
                );
                let events = terminal(&events);
                let last = events.last().unwrap();
                assert_eq!(last.kind, ChatEventKind::Completed, "{}", last.text);
                assert!(
                    core.messages(&id)
                        .unwrap()
                        .last()
                        .unwrap()
                        .content
                        .contains("LIVE_CHAT_OK")
                );
                chatted = true;
                println!("Chat completed after being sent during real background execution.");
            }
            if task.report_state == "private_assimilated" {
                assert_eq!(task.state, DelegationState::Completed, "{}", task.detail);
                assert!(chatted, "Did not exercise chat during the real task");
                assert!(observer.0.lock().unwrap().0, "No webSearch event observed");
                assert!(task.thread_id.is_some());
                assert!(
                    task.links
                        .iter()
                        .any(|link| link.target.starts_with("https://"))
                );
                assert_eq!(core.messages(&id).unwrap().len(), 2);
                let wakes: i64 = db
                    .query_row(
                        "SELECT wakes_used FROM persona_activity_runtime WHERE persona_id=?1",
                        [&id],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(wakes, 2);
                let summary = core.persona_activity(&id).unwrap().summary;
                assert!(!summary.trim().is_empty());
                println!(
                    "elapsed_seconds={} thread={:?} report={} wakes={} sources={:?}\nresult={}\nprivate_summary={summary}",
                    start_time.elapsed().as_secs_f32(),
                    task.thread_id,
                    task.report_state,
                    wakes,
                    task.links,
                    task.result
                );
                core.stop_activity(&id).unwrap();
                break;
            }
        }
        if start_time.elapsed() >= Duration::from_secs(300) {
            for task in &tasks {
                let _ = core.cancel_delegation(&task.id);
            }
            core.stop_activity(&id).unwrap();
            panic!("Real heartbeat did not settle within five minutes");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
