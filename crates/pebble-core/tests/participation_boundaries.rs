mod classifier_support;
mod group_support;
mod live_support;
use classifier_support::{Reply, Server, answer, configure, finish, submit};
use group_support::{count, db, envelope};
use pebble_core::{ChatEventKind, Error, ResponseAction, ResponseUpdate};
use serde_json::json;
use std::sync::{Mutex, mpsc};
use std::time::Duration;

fn pending_boundary(action: Option<ResponseAction>) {
    for api in ["chat_completions", "responses"] {
        let (root, core, persona) = live_support::isolated();
        // Preserve a real existing session, not just the absence of one.
        let (_task, rx) = live_support::start(&core, &persona, "Existing private context");
        assert_eq!(
            live_support::terminal(&rx).last().unwrap().kind,
            ChatEventKind::Completed
        );
        let (entered, received) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let released = Mutex::new(released);
        let server = Server::handling(move |_| {
            entered.send(()).unwrap();
            released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Reply::json(answer(api, "{\"respond\":true}"))
        });
        configure(&core, None, &server.url, api, "");
        let room = core
            .create_group("Pending boundary", std::slice::from_ref(&persona))
            .unwrap();
        let (submission, events) = submit(&core, &room.id, "ORIGINAL_PENDING_TRIGGER", &[]);
        received.recv_timeout(Duration::from_secs(2)).unwrap();
        let before = core.response_settings(&persona).unwrap();
        let directory = root.path().join("personas").join(&persona);
        let paths = [
            "config.toml",
            "response_schema.json",
            "SOUL.md",
            "MEMORY.md",
        ];
        let files = paths.map(|path| std::fs::read(directory.join(path)).unwrap());
        let storage = db(root.path());
        let tables = [
            "model_sessions",
            "model_items",
            "model_calls",
            "model_item_events",
        ];
        let counts = tables.map(|table| count(&storage, &format!("SELECT COUNT(*) FROM {table}")));
        let result = if let Some(action) = action {
            core.save_response_settings(
                &persona,
                &ResponseUpdate {
                    expected: before.clone(),
                    schema_json: before.schema_json.clone(),
                    initial_state_json: json!({"affinity_user":7,"mood":"quiet"}).to_string(),
                    action,
                },
            )
            .map(|_| ())
        } else {
            core.fresh_context(&persona).map(|_| ())
        };
        let after = core.response_settings(&persona).unwrap();
        let after_files = paths.map(|path| std::fs::read(directory.join(path)).unwrap());
        let after_counts =
            tables.map(|table| count(&storage, &format!("SELECT COUNT(*) FROM {table}")));
        // Release before assertions so the synthetic endpoint always finishes, even on regression.
        release.send(()).unwrap();
        let completed = finish(&submission, &events);
        assert!(
            matches!(result, Err(Error::Busy)),
            "{api} {action:?}: {result:?}"
        );
        assert_eq!(after, before);
        assert_eq!(after_files, files);
        assert_eq!(after_counts, counts);
        assert!(
            completed
                .iter()
                .any(|event| event.kind == ChatEventKind::Completed)
        );
        let call = core.participation(&room.id).unwrap()[0]
            .call_id
            .clone()
            .unwrap();
        let request = core.model_request(&call).unwrap();
        let input = envelope(&request);
        assert_eq!(
            input["reply_request"]["trigger_message_id"],
            submission.message.id
        );
        assert!(
            input["events"]
                .to_string()
                .contains("ORIGINAL_PENDING_TRIGGER")
        );
    }
}

#[test]
fn pending_classifier_blocks_fresh_context_without_losing_trigger() {
    pending_boundary(None);
}

#[test]
fn pending_classifier_blocks_initial_defaults_without_publishing_files() {
    pending_boundary(Some(ResponseAction::InitialDefaultsOnly));
}

#[test]
fn pending_classifier_blocks_fresh_context_keep_without_losing_trigger() {
    pending_boundary(Some(ResponseAction::FreshContextKeepState));
}

#[test]
fn pending_classifier_blocks_fresh_context_reset_without_mutating_state() {
    pending_boundary(Some(ResponseAction::FreshContextResetState));
}
