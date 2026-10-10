mod classifier_support;
mod group_support;
mod live_support;
mod responses_support;
use classifier_support::{Reply, Server, answer, configure, finish, submit};
use group_support::{count, db, task};
use pebble_core::{ChatEventKind, CredentialChange, SettingsUpdate};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn save_chat(core: &pebble_core::Core, persona: &str, url: &str, api: &str) {
    let mut settings = core.settings(Some(persona)).unwrap();
    settings.chat.enabled = true;
    settings.chat.context_len = Some(65536);
    settings.chat.max_output_tokens = Some(4096);
    settings.chat.base_url = url.into();
    settings.chat.model = "synthetic".into();
    settings.chat.api = api.into();
    core.save_settings(
        Some(persona),
        &SettingsUpdate {
            chat: settings.chat,
            credential: CredentialChange::Clear,
            persona: settings.persona,
        },
    )
    .unwrap();
}

#[test]
fn cancelled_queued_reassessment_terminates_decision_and_ignores_late_http_both_apis() {
    for api in ["chat_completions", "responses"] {
        for whole_round in [false, true] {
            let (root, core, persona) = live_support::isolated();
            let requests = AtomicUsize::new(0);
            let classifier = Server::handling(move |request| {
                let index = requests.fetch_add(1, Ordering::SeqCst);
                assert!(index < 2, "Cancelled decision restarted classification");
                let text = if api == "responses" {
                    request["input"][1]["content"][0]["text"].as_str()
                } else {
                    request["messages"][1]["content"].as_str()
                }
                .unwrap();
                let input: serde_json::Value = serde_json::from_str(text).unwrap();
                if index == 1 {
                    assert_eq!(
                        input["MEMORY_current_state"]["current_state"]["affinity_user"],
                        47
                    );
                }
                Reply::json(answer(api, "{\"respond\":true}")).delayed(if index == 1 {
                    600
                } else {
                    0
                })
            });
            configure(&core, None, &classifier.url, api, "");
            let private = Server::handling(move |_| {
                let text = json!({"reply":"Accepted private predecessor","state":{"affinity_user":47,"mood":"calm"}}).to_string();
                Reply::stream(if api == "responses" {
                    responses_support::candidate(&text)
                } else {
                    live_support::candidate(&text)
                })
                .delayed(350)
            });
            save_chat(&core, &persona, &private.url, api);
            let (_predecessor, private_rx) =
                live_support::start(&core, &persona, "Private changes state");
            private
                .requests
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            let room = core
                .create_group("Cancel reassessment", std::slice::from_ref(&persona))
                .unwrap();
            let (post, events) = submit(&core, &room.id, "Optional root", &[]);
            let admitted = task(&post, 0);
            classifier
                .requests
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            assert_eq!(
                live_support::terminal(&private_rx).last().unwrap().kind,
                ChatEventKind::Completed
            );
            classifier
                .requests
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            let pending = core.participation(&room.id).unwrap().remove(0);
            assert_eq!(pending.status, "pending");
            assert_eq!(pending.slot_status, "admitted");
            assert_eq!(pending.call_id.as_deref(), Some(admitted.id.as_str()));
            let state = core.response_settings(&persona).unwrap().current_state_json;
            let outputs = count(
                &db(root.path()),
                "SELECT COUNT(*) FROM model_items WHERE kind='output'",
            );
            assert!(outputs > 0);
            if whole_round {
                post.group.as_ref().unwrap().cancel();
            } else {
                admitted.cancel();
            }
            let terminal = finish(&post, &events);
            assert_eq!(
                terminal
                    .iter()
                    .filter(|e| e.kind == ChatEventKind::Cancelled)
                    .count(),
                1
            );
            let cancelled = core.participation(&room.id).unwrap().remove(0);
            assert_eq!(cancelled.slot_status, "cancelled");
            assert_eq!(cancelled.status, "cancelled");
            assert_eq!(cancelled.respond, None);
            core.fresh_context(&persona).unwrap();
            save_chat(&core, &persona, &private.url, api);
            let settings = core.classifier_settings(Some(&persona)).unwrap();
            core.save_classifier_settings(Some(&persona), &settings, &CredentialChange::Clear)
                .unwrap();
            core.stop_room(&room.id).unwrap();
            // Join the server only after cancellation settles: its delayed true body is late.
            drop(classifier);
            assert_eq!(core.participation(&room.id).unwrap().remove(0), cancelled);
            assert_eq!(
                core.response_settings(&persona).unwrap().current_state_json,
                state
            );
            core.fresh_context(&persona).unwrap();
            for (sql, expected) in [
                (
                    "SELECT COUNT(*) FROM participation_decisions WHERE status='pending'",
                    0,
                ),
                (
                    "SELECT COUNT(*) FROM group_reply_slots WHERE status IN ('pending','admitted')",
                    0,
                ),
                ("SELECT COUNT(*) FROM model_calls", 2),
                (
                    "SELECT COUNT(*) FROM model_items WHERE kind='output'",
                    outputs,
                ),
                ("SELECT COUNT(*) FROM persona_events WHERE kind='sent'", 1),
            ] {
                assert_eq!(count(&db(root.path()), sql), expected, "{sql}");
            }
            assert!(
                private.requests.try_recv().is_err(),
                "Cancelled optional generation started"
            );
        }
    }
}
