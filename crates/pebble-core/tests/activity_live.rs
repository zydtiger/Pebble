//! Explicit opt-in: synthetic persona HTTP output, real local Codex exploration.
mod live_support;
mod responses_support;
use live_support::*;
use pebble_core::{DelegatedTask, DelegationListener, DelegationState};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

struct SearchObserver(AtomicBool);
impl DelegationListener for SearchObserver {
    fn on_tasks(&self, tasks: Vec<DelegatedTask>) -> Result<(), String> {
        if tasks
            .iter()
            .any(|task| task.activity == "Searching the web…")
        {
            self.0.store(true, Ordering::Release);
        }
        Ok(())
    }
}

#[test]
#[ignore = "requires authenticated local Codex; real public web search from a synthetic private heartbeat"]
fn heartbeat_runs_real_codex_web_search_and_assimilates_privately() {
    let arguments = json!({"title":"Bread fermentation curiosity","instructions":"Use live web search to find one reliable public source explaining how lactic acid bacteria contribute to sourdough fermentation. Return two factual sentences and a precise source URL. This is a bounded public lookup: do not access local files, personal accounts or send external messages.","request_key":"sourdough-curiosity","task_id":null}).to_string();
    let call = format!(
        "data: {}\n\n",
        json!({"type":"response.completed","response":{"status":"completed","output":[{"id":"private-research","type":"function_call","status":"completed","call_id":"explore-fermentation","name":"delegate_task","arguments":arguments}]}})
    );
    let private = json!({"state":{"affinity_user":20,"mood":"calm"},"activity":{"summary":"Reviewing the fermentation discovery privately.","phase":"waiting","next_wake_minutes":30,"observed_outcomes":"$observed_outcomes","plans":null,"share_intent_id":null}}).to_string();
    let server = Server::memory(vec![
        stream(call),
        stream(responses_support::candidate(&private)),
        stream(responses_support::candidate(&private)),
    ]);
    let (root, core, id) = isolated();
    configure(
        root.path(),
        &server.url,
        "synthetic-persona",
        "responses",
        "",
    );
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("65536", "262144"),
    )
    .unwrap();
    core.save_codex_settings(&pebble_core::CodexSettings::default())
        .unwrap();
    let observer = Arc::new(SearchObserver(AtomicBool::new(false)));
    core.observe_delegations(observer.clone()).unwrap();
    let mut settings = core.activity_settings(&id).unwrap();
    settings.enabled = true;
    settings.quiet_start_minutes = None;
    settings.quiet_end_minutes = None;
    core.save_activity_settings(&id, &settings).unwrap();
    let start = Instant::now();
    loop {
        let tasks = core.delegated_tasks().unwrap();
        if let Some(task) = tasks.first()
            && task.report_state == "private_assimilated"
        {
            println!(
                "elapsed_seconds={} state={:?} report={} observed_web_search={} sources={:?}\n{}",
                start.elapsed().as_secs_f32(),
                task.state,
                task.report_state,
                observer.0.load(Ordering::Acquire),
                task.links,
                task.result
            );
            assert_eq!(task.state, DelegationState::Completed);
            assert!(
                observer.0.load(Ordering::Acquire),
                "no real webSearch event observed"
            );
            assert!(
                task.links
                    .iter()
                    .any(|link| link.target.starts_with("https://"))
            );
            assert!(
                core.room_messages(&task.conversation_id)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(tasks.len(), 1);
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(180),
            "background exploration did not settle: {tasks:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let inputs: Vec<_> = server
        .requests
        .try_iter()
        .map(|(_, value)| value.to_string())
        .collect();
    assert_eq!(inputs.len(), 3);
    assert!(inputs[2].contains("task_outcomes"));
    assert!(inputs[2].contains("https://"));
    core.stop_activity(&id).unwrap();
}
