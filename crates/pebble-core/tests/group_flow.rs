mod classifier_support;
mod group_support;
mod live_support;
mod responses_support;
use classifier_support::{Reply, Server, answer, configure, finish, submit};
use group_support::{count, db};
use live_support::isolated;
use pebble_core::ChatEventKind;
use serde_json::Value;
use std::time::Duration;

fn chat(core: &pebble_core::Core, persona: &str, url: &str, api: &str) {
    let mut s = core.settings(Some(persona)).unwrap();
    s.chat.enabled = true;
    s.chat.model = "synthetic-chat".into();
    s.chat.base_url = url.into();
    s.chat.api = api.into();
    core.save_settings(
        Some(persona),
        &pebble_core::SettingsUpdate {
            chat: s.chat,
            credential: pebble_core::CredentialChange::Clear,
            context_window_tokens: s.context_window_tokens,
            max_output_tokens: s.max_output_tokens,
            persona: s.persona,
        },
    )
    .unwrap();
}
fn content<'a>(request: &'a Value, api: &str, index: usize) -> &'a str {
    if api == "responses" {
        request["input"][index]["content"][0]["text"]
            .as_str()
            .unwrap()
    } else {
        request["messages"][index]["content"].as_str().unwrap()
    }
}
fn generated(api: &str, text: &str) -> Reply {
    let text = live_support::structured(text);
    Reply::stream(if api == "responses" {
        responses_support::candidate(&text)
    } else {
        live_support::candidate(&text)
    })
}
fn admissions(events: &[pebble_core::ChatEvent]) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.kind == ChatEventKind::Admitted)
        .map(|e| e.persona_id.clone())
        .collect()
}

#[test]
fn deterministic_initial_order_then_two_followups_bot_mentions_never_force_both_apis() {
    for api in ["chat_completions", "responses"] {
        let (root, core, a) = isolated();
        let b = core.create_persona("Kai").unwrap().id;
        let c = core.create_persona("Silent").unwrap().id;
        let silent = c.clone();
        let slow = a.clone();
        let classifier = Server::handling(move |request| {
            let input: Value = serde_json::from_str(content(request, api, 1)).unwrap();
            let id = input["persona"]["id"].as_str().unwrap();
            Reply::json(answer(
                api,
                if id == silent {
                    "{\"respond\":false}"
                } else {
                    "{\"respond\":true}"
                },
            ))
            .delayed(if id == slow { 80 } else { 0 })
        });
        configure(&core, None, &classifier.url, api, "");
        let generation = Server::handling(move |_| generated(api, "Accepted public reply @Silent"));
        for id in [&a, &b, &c] {
            chat(&core, id, &generation.url, api);
        }
        let room = core
            .create_group("Finite flow", &[a.clone(), b.clone(), c.clone()])
            .unwrap();
        let (post, rx) = submit(&core, &room.id, "Initial discussion", &[]);
        let events = finish(&post, &rx);
        assert_eq!(
            admissions(&events),
            vec![a.clone(), b.clone(), a.clone(), b.clone()]
        );
        assert_eq!(
            count(&db(root.path()), "SELECT followups_used FROM group_rounds"),
            2
        );
        assert_eq!(
            count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
            4
        );
        assert_eq!(
            count(
                &db(root.path()),
                "SELECT COUNT(*) FROM messages WHERE sender_kind='user'"
            ),
            1
        );
        assert_eq!(
            count(
                &db(root.path()),
                "SELECT COUNT(*) FROM group_reply_slots WHERE status IN ('pending','admitted')"
            ),
            0
        );
        assert_eq!(core.room_messages(&room.id).unwrap().len(), 5);
        // The next round starts with the least recently accepted speaker, never placeholder order.
        let (next, rx) = submit(&core, &room.id, "Next discussion", &[]);
        let events = finish(&next, &rx);
        assert_eq!(
            admissions(&events),
            vec![a.clone(), b.clone(), a.clone(), b.clone()]
        );
        assert!(
            generation
                .requests
                .try_iter()
                .all(|(_, request)| request["stream"] == true)
        );
    }
}

#[test]
fn all_initial_members_can_reply_required_first_and_optional_rechecks_predecessor() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap().id;
    let c = core.create_persona("Nova").unwrap().id;
    let classifier =
        Server::handling(|_| Reply::json(answer("chat_completions", "{\"respond\":true}")));
    configure(&core, None, &classifier.url, "chat_completions", "");
    let room = core
        .create_group("No initial cap", &[a.clone(), b.clone(), c.clone()])
        .unwrap();
    let (post, rx) = submit(
        &core,
        &room.id,
        "Request Nova first",
        std::slice::from_ref(&c),
    );
    let events = finish(&post, &rx);
    let order = admissions(&events);
    assert_eq!(&order[..3], &[c.clone(), a.clone(), b.clone()]);
    assert_eq!(order.len(), 5);
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM group_reply_slots WHERE kind IN ('required','optional') AND status='completed'"
        ),
        3
    );
    let requests = classifier
        .requests
        .try_iter()
        .map(|(_, request)| request)
        .collect::<Vec<_>>();
    assert!(
        requests
            .iter()
            .any(|r| content(r, "chat_completions", 1).contains("local demo"))
    );
}

#[test]
fn split_fallback_uses_frozen_first_connection_own_permission_and_ends_after_one_both_apis() {
    for api in ["chat_completions", "responses"] {
        let (root, core, a) = isolated();
        let b = core.create_persona("Kai").unwrap().id;
        let first = Server::handling(move |request| {
            let system = content(request, api, 0);
            if system == include_str!("../assets/fallback-intent-prompt.txt").trim_end() {
                let input: Value = serde_json::from_str(content(request, api, 1)).unwrap();
                assert_eq!(input.as_object().unwrap().len(), 1);
                assert!(input.get("SOUL").is_none());
                Reply::json(answer(api, "{\"whole_group_question\":true}"))
            } else {
                Reply::json(answer(api, "{\"respond\":false}"))
            }
        });
        let second = Server::handling(move |request| {
            let system = content(request, api, 0);
            assert_ne!(
                system,
                include_str!("../assets/fallback-intent-prompt.txt").trim_end()
            );
            let permitted =
                system == include_str!("../assets/fallback-permission-prompt.txt").trim_end();
            if permitted {
                let input: Value = serde_json::from_str(content(request, api, 1)).unwrap();
                assert!(input.get("SOUL").is_none());
                assert!(input.get("recent_real_user_transcript").is_some());
            }
            Reply::json(answer(
                api,
                if permitted {
                    "{\"respond\":true}"
                } else {
                    "{\"respond\":false}"
                },
            ))
        });
        configure(&core, Some(&a), &first.url, api, "");
        configure(&core, Some(&b), &second.url, api, "");
        let generation = Server::handling(move |_| generated(api, "One acknowledgment"));
        chat(&core, &a, &generation.url, api);
        chat(&core, &b, &generation.url, api);
        let room = core
            .create_group("Whole room", &[a.clone(), b.clone()])
            .unwrap();
        let (post, rx) = submit(&core, &room.id, "Does everyone agree?", &[]);
        assert_eq!(admissions(&finish(&post, &rx)), vec![b]);
        assert_eq!(
            count(&db(root.path()), "SELECT followups_used FROM group_rounds"),
            0
        );
        assert_eq!(
            count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
            1
        );
        assert_eq!(
            count(
                &db(root.path()),
                "SELECT COUNT(*) FROM group_rounds WHERE status='completed' AND fallback_stage='done'"
            ),
            1
        );
        let requests = first
            .requests
            .try_iter()
            .map(|(_, r)| r)
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 3); // normal false, room intent, this member's permission false.
        for request in requests.iter().chain(
            second
                .requests
                .try_iter()
                .map(|(_, r)| r)
                .collect::<Vec<_>>()
                .iter(),
        ) {
            let format = if api == "responses" {
                &request["text"]["format"]
            } else {
                &request["response_format"]["json_schema"]
            };
            assert_eq!(format["strict"], true);
            assert_eq!(format["schema"]["additionalProperties"], false);
        }
    }
}

#[test]
fn fallback_false_error_and_silence_do_not_admit_or_force() {
    for mode in [
        "intent_false",
        "intent_error",
        "permission_false",
        "permission_error",
        "primary_error",
    ] {
        let (root, core, a) = isolated();
        let server = Server::handling(move |request| {
            let system = content(request, "chat_completions", 0);
            let text = if system == include_str!("../assets/participation-prompt.txt").trim_end() {
                if mode == "primary_error" {
                    "invalid"
                } else {
                    "{\"respond\":false}"
                }
            } else if system == include_str!("../assets/fallback-intent-prompt.txt").trim_end() {
                match mode {
                    "intent_false" => "{\"whole_group_question\":false}",
                    "intent_error" => "invalid",
                    _ => "{\"whole_group_question\":true}",
                }
            } else {
                if mode == "permission_error" {
                    "invalid"
                } else {
                    "{\"respond\":false}"
                }
            };
            Reply::json(answer("chat_completions", text))
        });
        configure(&core, None, &server.url, "chat_completions", "");
        let room = core
            .create_group("Quiet room", std::slice::from_ref(&a))
            .unwrap();
        let (post, rx) = submit(&core, &room.id, "Please stay quiet", &[]);
        assert!(admissions(&finish(&post, &rx)).is_empty());
        assert_eq!(
            count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
            0
        );
        assert_eq!(
            count(
                &db(root.path()),
                "SELECT COUNT(*) FROM group_reply_slots WHERE status IN ('pending','admitted')"
            ),
            0,
            "Silent terminal fallback must release persona idle guards"
        );
        core.fresh_context(&a).unwrap();
        let settings = core.settings(Some(&a)).unwrap();
        core.save_settings(
            Some(&a),
            &pebble_core::SettingsUpdate {
                chat: settings.chat,
                credential: pebble_core::CredentialChange::Keep,
                context_window_tokens: settings.context_window_tokens,
                max_output_tokens: settings.max_output_tokens,
                persona: settings.persona,
            },
        )
        .unwrap();
        let settings = core.classifier_settings(None).unwrap();
        core.save_classifier_settings(None, &settings, &pebble_core::CredentialChange::Keep)
            .unwrap();
        assert_eq!(
            count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
            0
        );
        assert_eq!(
            count(
                &db(root.path()),
                "SELECT COUNT(*) FROM group_reply_slots WHERE status IN ('pending','admitted')"
            ),
            0
        );
        let n = server.requests.try_iter().count();
        assert_eq!(
            n,
            if mode == "primary_error" {
                1
            } else if mode.starts_with("intent_") {
                2
            } else {
                3
            }
        );
        if mode.ends_with("error") {
            assert_eq!(core.participation(&room.id).unwrap()[0].status, "failed");
        }
    }
}

#[test]
fn new_input_cancels_admitted_optional_without_state_native_or_publication_acceptance() {
    let (root, core, a) = isolated();
    let target = a.clone();
    let classifier = Server::handling(move |request| {
        let system = content(request, "chat_completions", 0);
        let input: Value = serde_json::from_str(content(request, "chat_completions", 1)).unwrap();
        let yes = input["recent_group_transcript"]
            .as_array()
            .and_then(|v| v.last())
            .is_some_and(|v| v["text"] == "OLD_OPTIONAL")
            && !system.contains("whole_group_question");
        Reply::json(answer(
            "chat_completions",
            if system == include_str!("../assets/fallback-intent-prompt.txt").trim_end() {
                "{\"whole_group_question\":false}"
            } else if yes {
                "{\"respond\":true}"
            } else {
                "{\"respond\":false}"
            },
        ))
    });
    configure(&core, None, &classifier.url, "chat_completions", "");
    let generation =
        Server::handling(|_| generated("chat_completions", "Late unacceptable reply").delayed(250));
    chat(&core, &a, &generation.url, "chat_completions");
    let room = core
        .create_group("Interrupt", std::slice::from_ref(&a))
        .unwrap();
    let before = core.response_settings(&a).unwrap();
    let (old, rx) = submit(&core, &room.id, "OLD_OPTIONAL", &[]);
    generation
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let (new, newrx) = submit(&core, &room.id, "NEW_USER_INPUT", &[]);
    finish(&old, &rx);
    finish(&new, &newrx);
    assert_eq!(
        core.response_settings(&target).unwrap().current_state_json,
        before.current_state_json
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM model_items WHERE kind='output'"
        ),
        0
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"
        ),
        0
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM messages WHERE sender_kind='persona' AND status='complete'"
        ),
        0
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM persona_events WHERE kind='received'"
        ),
        2
    );
}

#[test]
fn failed_followup_keeps_spent_budget_and_explicit_retry_never_resumes_chain() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap().id;
    let classifier =
        Server::handling(|_| Reply::json(answer("chat_completions", "{\"respond\":true}")));
    configure(&core, None, &classifier.url, "chat_completions", "");
    let n = Arc::new(AtomicUsize::new(0));
    let calls = n.clone();
    let generation = Server::handling(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 2 {
            let mut r = Reply::json(serde_json::json!({"error":"synthetic transport failure"}));
            r.status = 500;
            r
        } else {
            generated("chat_completions", "Accepted finite reply")
        }
    });
    for id in [&a, &b] {
        chat(&core, id, &generation.url, "chat_completions");
    }
    let room = core
        .create_group("Budget failure", &[a.clone(), b])
        .unwrap();
    let (post, rx) = submit(&core, &room.id, "Finite root", &[]);
    let events = finish(&post, &rx);
    assert_eq!(admissions(&events).len(), 3);
    assert_eq!(
        count(&db(root.path()), "SELECT followups_used FROM group_rounds"),
        1
    );
    let last = core.room_messages(&room.id).unwrap().pop().unwrap();
    assert!(last.can_retry);
    let (_retry, rx) = live_support::retry(&core, &a, &last.id);
    assert_eq!(
        live_support::terminal(&rx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    assert_eq!(
        count(&db(root.path()), "SELECT followups_used FROM group_rounds"),
        1
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM group_rounds WHERE status='completed' AND continuation=0"
        ),
        1
    );
    assert_eq!(n.load(Ordering::SeqCst), 4);
}

#[test]
fn structured_format_repair_stays_inside_one_admission_and_the_same_budget() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap().id;
    let classifier =
        Server::handling(|_| Reply::json(answer("chat_completions", "{\"respond\":true}")));
    configure(&core, None, &classifier.url, "chat_completions", "");
    let n = AtomicUsize::new(0);
    let generation = Server::handling(move |_| {
        if n.fetch_add(1, Ordering::SeqCst) == 2 {
            Reply::stream(live_support::candidate("malformed candidate"))
        } else {
            generated("chat_completions", "Accepted after repair")
        }
    });
    for id in [&a, &b] {
        chat(&core, id, &generation.url, "chat_completions");
    }
    let room = core.create_group("Repair budget", &[a, b]).unwrap();
    let (post, rx) = submit(&core, &room.id, "Finite root", &[]);
    let events = finish(&post, &rx);
    assert_eq!(admissions(&events).len(), 4);
    assert_eq!(
        count(&db(root.path()), "SELECT followups_used FROM group_rounds"),
        2
    );
    assert_eq!(
        count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
        4
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM model_items WHERE kind='output'"
        ),
        4
    );
    assert_eq!(generation.requests.try_iter().count(), 5);
}
