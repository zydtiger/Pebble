mod live_support;
use live_support::*;
use pebble_core::{CodexSettings, CredentialChange, DelegationState, SettingsUpdate};
use std::time::{Duration, Instant};
#[test]
#[ignore = "requires explicitly selected live persona endpoint and logged-in local Codex; synthetic web lookup only"]
fn actual_persona_both_protocols_delegate_and_explain_saved_web_result() {
    let url = std::env::var("PEBBLE_LIVE_API_ROOT").expect("set PEBBLE_LIVE_API_ROOT");
    let model = std::env::var("PEBBLE_LIVE_MODEL").expect("set PEBBLE_LIVE_MODEL");
    for api in ["chat_completions", "responses"] {
        let (root, core, id) = isolated();
        configure(root.path(), &url, &model, api, "");
        let mut settings = core.settings(None).unwrap();
        settings.chat.max_output_tokens = Some(2048);
        settings.chat.request_json =
            r#"{"temperature":0,"chat_template_kwargs":{"enable_thinking":false}}"#.into();
        if api == "responses" {
            settings.chat.responses_schema_format = "response_format".into();
        }
        core.save_settings(
            None,
            &SettingsUpdate {
                chat: settings.chat,
                credential: CredentialChange::Keep,
                persona: None,
            },
        )
        .unwrap();
        core.save_codex_settings(&CodexSettings::default()).unwrap();
        let started = Instant::now();
        let (_handle, events) = start(
            &core,
            &id,
            "Please use delegate_task to ask Codex for one bounded web lookup: verify the current stable Rust version from the official rust-lang.org site, and return a precise source URL. Acknowledge the task promptly; do not invent the lookup or answer from memory.",
        );
        let events = terminal(&events);
        assert_eq!(
            events.last().unwrap().kind,
            pebble_core::ChatEventKind::Completed,
            "{api}: {}",
            events.last().unwrap().text
        );
        loop {
            let tasks = core.delegated_tasks().unwrap();
            if tasks.len() == 1 && ["completed", "failed"].contains(&tasks[0].report_state.as_str())
            {
                let task = &tasks[0];
                println!(
                    "api={api} elapsed_seconds={} state={:?} report={} sources={:?}\nactual_result={}\nactual_persona_reply={}",
                    started.elapsed().as_secs_f32(),
                    task.state,
                    task.report_state,
                    task.links,
                    task.result,
                    core.messages(&id).unwrap().last().unwrap().content
                );
                assert_eq!(task.state, DelegationState::Completed);
                assert_eq!(task.report_state, "completed");
                assert!(task.links.iter().any(|l| l.target.starts_with("https://")));
                assert!(!task.result.contains('\u{e200}'));
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(180),
                "{api} timed out or did not delegate: {tasks:?}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
