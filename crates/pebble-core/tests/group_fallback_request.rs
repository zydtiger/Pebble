mod classifier_support;
mod group_support;
mod live_support;
mod responses_support;
use classifier_support::{Reply, Server, answer, configure, finish, submit};
use group_support::{count, db};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

fn envelope(request: &Value, api: &str) -> Value {
    let items = request[if api == "responses" {
        "input"
    } else {
        "messages"
    }]
    .as_array()
    .unwrap();
    items
        .iter()
        .rev()
        .filter(|item| item["role"] == "user")
        .filter_map(|item| {
            let text = if api == "responses" {
                item["content"][0]["text"].as_str()
            } else {
                item["content"].as_str()
            }?;
            serde_json::from_str::<Value>(text).ok()
        })
        .find(|value| value.get("reply_request").is_some())
        .unwrap()
}

#[test]
fn fallback_purpose_is_frozen_for_repairs_and_retry_without_changing_normal_calls_both_apis() {
    for api in ["chat_completions", "responses"] {
        for fresh in [false, true] {
            let (root, core, persona) = live_support::isolated();
            let classifier = Server::handling(move |request| {
                let system = if api == "responses" {
                    request["input"][0]["content"][0]["text"].as_str()
                } else {
                    request["messages"][0]["content"].as_str()
                }
                .unwrap();
                Reply::json(answer(
                    api,
                    if system == include_str!("../assets/fallback-intent-prompt.txt").trim_end() {
                        "{\"whole_group_question\":true}"
                    } else if system
                        == include_str!("../assets/fallback-permission-prompt.txt").trim_end()
                    {
                        "{\"respond\":true}"
                    } else {
                        "{\"respond\":false}"
                    },
                ))
            });
            configure(&core, None, &classifier.url, api, "");
            let candidates = AtomicUsize::new(0);
            let generation = Server::handling(move |_| {
                let text = if candidates.fetch_add(1, Ordering::SeqCst) < 3 {
                    "malformed".into()
                } else {
                    live_support::structured("Accepted reply")
                };
                Reply::stream(if api == "responses" {
                    responses_support::candidate(&text)
                } else {
                    live_support::candidate(&text)
                })
            });
            let mut settings = core.settings(Some(&persona)).unwrap();
            settings.chat.enabled = true;
            settings.chat.model = "synthetic-chat".into();
            settings.chat.base_url = generation.url.clone();
            settings.chat.api = api.into();
            core.save_settings(
                Some(&persona),
                &pebble_core::SettingsUpdate {
                    chat: settings.chat,
                    credential: pebble_core::CredentialChange::Clear,
                    context_window_tokens: settings.context_window_tokens,
                    max_output_tokens: settings.max_output_tokens,
                    persona: settings.persona,
                },
            )
            .unwrap();
            let room = core
                .create_group("Acknowledgment purpose", std::slice::from_ref(&persona))
                .unwrap();
            let (post, rx) = submit(&core, &room.id, "Does everyone agree?", &[]);
            finish(&post, &rx);
            let failed = core.room_messages(&room.id).unwrap().pop().unwrap();
            assert!(failed.can_retry);
            let originals = generation.requests.try_iter().collect::<Vec<_>>();
            assert_eq!(originals.len(), 3);
            let frozen = envelope(&originals[0].1, api);
            assert_eq!(frozen["reply_request"]["purpose"], "group_acknowledgment");
            let instructions = frozen["reply_request"]["instructions"].as_str().unwrap();
            assert!(instructions.contains("brief acknowledgment or clarification"));
            assert!(instructions.contains("this reply only"));
            for (_, candidate) in &originals[1..] {
                assert_eq!(envelope(candidate, api), frozen);
            }
            if fresh {
                core.fresh_context(&persona).unwrap();
            }
            let (_retry, rx) = live_support::retry(&core, &persona, &failed.id);
            live_support::terminal(&rx);
            let (_, retry) = generation.requests.recv().unwrap();
            let retried = envelope(&retry, api);
            assert_eq!(retried["reply_request"], frozen["reply_request"]);
            if !fresh {
                assert_eq!(retry, originals[0].1);
            }
            assert_eq!(
                count(&db(root.path()), "SELECT COUNT(*) FROM group_rounds"),
                1
            );
            assert_eq!(
                count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
                2
            );
            assert_eq!(
                count(&db(root.path()), "SELECT followups_used FROM group_rounds"),
                0
            );
            let (normal, rx) = submit(
                &core,
                &room.id,
                "Please give your ordinary reply",
                std::slice::from_ref(&persona),
            );
            finish(&normal, &rx);
            let (_, normal) = generation.requests.recv().unwrap();
            let ordinary = envelope(&normal, api);
            let key = if api == "responses" {
                "input"
            } else {
                "messages"
            };
            let prefix = retry[key].as_array().unwrap();
            assert_eq!(&normal[key].as_array().unwrap()[..prefix.len()], prefix);
            assert!(ordinary["reply_request"].get("purpose").is_none());
            assert!(ordinary["reply_request"].get("instructions").is_none());
            assert_eq!(ordinary["reply_request"]["conversation_id"], room.id);
        }
    }
}
