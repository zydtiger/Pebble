mod classifier_support;
mod group_support;
mod live_support;
use classifier_support::*;
use group_support::{count, db};
use live_support::{isolated, start, terminal};
use pebble_core::{ChatEventKind, MessageStatus};
use serde_json::{Value, json};
use std::time::Duration;

#[test]
fn false_reads_are_private_bounded_nonconsuming_and_both_protocol_requests_are_strict() {
    for (api, dialect) in [
        ("chat_completions", "text_format"),
        ("responses", "text_format"),
        ("responses", "response_format"),
    ] {
        let (root, core, a) = isolated();
        let b = core.create_persona("Kai").unwrap();
        let (_task, rx) = start(&core, &a, "A_PRIVATE_SECRET");
        terminal(&rx);
        let (_task, rx) = start(&core, &b.id, "B_PRIVATE_SECRET");
        terminal(&rx);
        let server = Server::new(vec![
            Reply::json(answer(api, "{\"respond\":false}")),
            Reply::json(answer(api, "{\"respond\":false}")),
            Reply::json(answer(api, "{\"whole_group_question\":false}")),
        ]);
        configure(
            &core,
            None,
            &server.url,
            api,
            if api == "chat_completions" {
                "qwen_non_thinking"
            } else {
                ""
            },
        );
        let mut settings = core.classifier_settings(None).unwrap();
        settings.connection.responses_schema_format = dialect.into();
        core.save_classifier_settings(None, &settings, &pebble_core::CredentialChange::Keep)
            .unwrap();
        let group = core
            .create_group("Private views", &[a.clone(), b.id.clone()])
            .unwrap();
        let storage = db(root.path());
        let tables = [
            "model_items",
            "model_item_events",
            "model_calls",
            "persona_events",
        ];
        let before = tables.map(|t| count(&storage, &format!("SELECT COUNT(*) FROM {t}")));
        let state_a = core.response_settings(&a).unwrap();
        let state_b = core.response_settings(&b.id).unwrap();
        let (submission, rx) = submit(&core, &group.id, "LATEST_GROUP_PUBLIC_INPUT", &[]);
        let events = finish(&submission, &rx);
        assert!(submission.tasks.is_empty());
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.kind, ChatEventKind::Admitted | ChatEventKind::Completed))
        );
        assert_eq!(core.room_messages(&group.id).unwrap().len(), 1);
        for d in core.participation(&group.id).unwrap() {
            assert_eq!(d.status, "listening");
            assert_eq!(d.respond, Some(false));
        }
        let after = tables.map(|t| count(&storage, &format!("SELECT COUNT(*) FROM {t}")));
        assert_eq!(&before[..3], &after[..3]);
        assert_eq!(after[3], before[3] + 2);
        assert_eq!(core.response_settings(&a).unwrap(), state_a);
        assert_eq!(core.response_settings(&b.id).unwrap(), state_b);
        for _ in 0..2 {
            let (head, request) = server
                .requests
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            assert!(!head.to_lowercase().contains("authorization:"));
            assert_eq!(request["temperature"], 0);
            assert_eq!(request["stream"], false);
            let messages = if api == "responses" {
                assert_eq!(request["max_output_tokens"], 64);
                &request["input"]
            } else {
                assert_eq!(request["max_tokens"], 64);
                assert_eq!(request["chat_template_kwargs"]["enable_thinking"], false);
                &request["messages"]
            };
            if api == "responses" {
                assert!(request.get("chat_template_kwargs").is_none());
            }
            let format = if api == "chat_completions" || dialect == "response_format" {
                &request["response_format"]["json_schema"]
            } else {
                &request["text"]["format"]
            };
            assert_eq!(format["strict"], true);
            assert_eq!(format["schema"]["required"], json!(["respond"]));
            assert_eq!(format["schema"]["additionalProperties"], false);
            let text = if api == "responses" {
                messages[1]["content"][0]["text"].as_str().unwrap()
            } else {
                messages[1]["content"].as_str().unwrap()
            };
            let input: Value = serde_json::from_str(text).unwrap();
            let target = input["persona"]["id"].as_str().unwrap();
            assert!(text.contains(if target == a {
                "A_PRIVATE_SECRET"
            } else {
                "B_PRIVATE_SECRET"
            }));
            assert!(!text.contains(if target == a {
                "B_PRIVATE_SECRET"
            } else {
                "A_PRIVATE_SECRET"
            }));
            assert_eq!(
                input["recent_group_transcript"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["text"],
                "LATEST_GROUP_PUBLIC_INPUT"
            );
            assert!(!text.contains("reasoning"));
            assert!(text.len() < 32 * 1024);
            let system = if api == "responses" {
                messages[0]["content"][0]["text"].as_str().unwrap()
            } else {
                messages[0]["content"].as_str().unwrap()
            };
            assert_eq!(
                system,
                include_str!("../assets/participation-prompt.txt").trim_end()
            );
        }
    }
}

#[test]
fn mentions_bypass_inference_and_volunteers_admit_once_without_duplicate_users() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap();
    let volunteer = b.id.clone();
    let server = Server::handling(move |request| {
        let view: Value =
            serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
        Reply::json(answer(
            "chat_completions",
            if view["persona"]["id"] == volunteer {
                "{\"respond\":true}"
            } else {
                "{\"respond\":false}"
            },
        ))
    });
    configure(&core, None, &server.url, "chat_completions", "");
    let group = core
        .create_group("Mention plus volunteer", &[a.clone(), b.id.clone()])
        .unwrap();
    let (submission, rx) = submit(
        &core,
        &group.id,
        "A direct request with room for Kai",
        std::slice::from_ref(&a),
    );
    assert!(submission.tasks.is_empty());
    let events = finish(&submission, &rx);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == ChatEventKind::Admitted)
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == ChatEventKind::Completed)
            .count(),
        2
    );
    let admitted = events
        .iter()
        .position(|e| e.kind == ChatEventKind::Admitted)
        .unwrap();
    let settled = events
        .iter()
        .position(|e| e.kind == ChatEventKind::GroupSettled)
        .unwrap();
    assert!(admitted < settled);
    let messages = core.room_messages(&group.id).unwrap();
    assert_eq!(
        messages.iter().filter(|m| m.sender_kind == "user").count(),
        1
    );
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.sender_kind == "persona" && m.status == MessageStatus::Complete)
            .count(),
        2
    );
    let request = server
        .requests
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .1;
    let input: Value =
        serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(input["persona"]["id"], b.id);
    assert_eq!(server.requests.try_iter().count(), 2);
    let storage = db(root.path());
    assert_eq!(count(&storage, "SELECT COUNT(*) FROM model_calls"), 2);
    assert_eq!(
        count(
            &storage,
            "SELECT COUNT(*) FROM participation_decisions WHERE status='selected' AND respond=1"
        ),
        1
    );
}

#[test]
fn malformed_refused_empty_truncated_and_extra_fields_are_visible_errors_without_repair_or_chat_calls()
 {
    for api in ["chat_completions", "responses"] {
        for text in [
            "not json",
            "{}",
            "{\"respond\":\"true\"}",
            "{\"respond\":true,\"extra\":0}",
            "{\"respond\":true,\"respond\":false}",
            "",
        ] {
            let (root, core, a) = isolated();
            let server = Server::new(vec![Reply::json(answer(api, text))]);
            configure(&core, None, &server.url, api, "");
            let group = core
                .create_group("Invalid boolean", std::slice::from_ref(&a))
                .unwrap();
            let (submission, rx) = submit(&core, &group.id, "Synthetic malformed check", &[]);
            finish(&submission, &rx);
            let decision = core.participation(&group.id).unwrap().remove(0);
            assert_eq!(decision.status, "failed");
            assert!(!decision.error.is_empty());
            assert!(decision.respond.is_none());
            assert_eq!(core.room_messages(&group.id).unwrap().len(), 1);
            assert_eq!(
                count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
                0
            );
            server
                .requests
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
            assert!(server.requests.try_recv().is_err());
        }
        for refusal in [false, true] {
            let (_root, core, a) = isolated();
            let mut body = answer(api, "{\"respond\":true}");
            if api == "responses" {
                if refusal {
                    body["output"][0]["content"] =
                        json!([{"type":"refusal","refusal":"Synthetic refusal"}]);
                } else {
                    body["status"] = json!("incomplete");
                }
            } else if refusal {
                body["choices"][0]["message"]["refusal"] = json!("Synthetic refusal");
            } else {
                body["choices"][0]["finish_reason"] = json!("length");
            }
            let server = Server::new(vec![Reply::json(body)]);
            configure(&core, None, &server.url, api, "");
            let group = core
                .create_group("Terminal outcomes", std::slice::from_ref(&a))
                .unwrap();
            let (submission, rx) = submit(&core, &group.id, "Terminal check", &[]);
            finish(&submission, &rx);
            assert_eq!(core.participation(&group.id).unwrap()[0].status, "failed");
            assert_eq!(core.room_messages(&group.id).unwrap().len(), 1);
        }
    }
}

#[test]
fn cancellation_stale_callbacks_and_restart_keep_observations_and_manual_recovery() {
    let (root, core, a) = isolated();
    let server = Server::new(vec![
        Reply::json(answer("chat_completions", "{\"respond\":true}")).delayed(400),
        Reply::json(answer("chat_completions", "{\"respond\":true}")).delayed(400),
    ]);
    configure(&core, None, &server.url, "chat_completions", "");
    let group = core
        .create_group("Stopped check", std::slice::from_ref(&a))
        .unwrap();
    let (submission, rx) = submit(&core, &group.id, "PUBLIC_CANCELLED_OBSERVATION", &[]);
    server
        .requests
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let decision = submission.participation[0].id.clone();
    submission
        .group
        .as_ref()
        .unwrap()
        .cancel_decision(&decision);
    finish(&submission, &rx);
    assert_eq!(
        core.participation(&group.id).unwrap()[0].status,
        "cancelled"
    );
    assert_eq!(core.room_messages(&group.id).unwrap().len(), 1);
    let (old, rx_old) = submit(&core, &group.id, "PUBLIC_STALE_OBSERVATION", &[]);
    server
        .requests
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let (manual, rx) = submit(
        &core,
        &group.id,
        "Mention recovery",
        std::slice::from_ref(&a),
    );
    finish(&manual, &rx);
    finish(&old, &rx_old);
    assert_eq!(
        count(&db(root.path()), "SELECT COUNT(*) FROM model_calls"),
        1
    );
    assert_eq!(core.room_messages(&group.id).unwrap().len(), 4);
    let request = core
        .model_request(&group_support::task(&manual, 0).id)
        .unwrap();
    let native = group_support::native(&request);
    assert!(
        native.contains("PUBLIC_CANCELLED_OBSERVATION")
            && native.contains("PUBLIC_STALE_OBSERVATION")
    );
    drop((submission, old, manual, core));
    std::thread::sleep(Duration::from_millis(30));
    let storage = db(root.path());
    storage
        .execute(
            "UPDATE participation_decisions SET status='pending',error='' WHERE id=?1",
            [&decision],
        )
        .unwrap();
    let core = pebble_core::Core::open(root.path()).unwrap();
    assert_eq!(
        core.participation(&group.id)
            .unwrap()
            .iter()
            .find(|d| d.id == decision)
            .unwrap()
            .status,
        "interrupted"
    );
    assert!(server.requests.try_recv().is_err());
    let (manual, rx) = submit(
        &core,
        &group.id,
        "After restart direct request",
        std::slice::from_ref(&a),
    );
    finish(&manual, &rx);
    assert_eq!(
        core.room_messages(&group.id)
            .unwrap()
            .last()
            .unwrap()
            .status,
        MessageStatus::Complete
    );
}

#[test]
fn classifier_settings_observe_pending_work_and_listener_drop_cancels_without_blank_bubble() {
    let (root, core, a) = isolated();
    let server = Server::new(vec![
        Reply::json(answer("chat_completions", "{\"respond\":true}")).delayed(400),
    ]);
    configure(&core, None, &server.url, "chat_completions", "");
    let group = core
        .create_group("Closed observer", std::slice::from_ref(&a))
        .unwrap();
    let (submission, rx) = submit(&core, &group.id, "Public before close", &[]);
    server
        .requests
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    let settings = core.classifier_settings(None).unwrap();
    assert!(matches!(
        core.save_classifier_settings(None, &settings, &pebble_core::CredentialChange::Keep),
        Err(pebble_core::Error::Busy)
    ));
    drop(rx);
    drop(submission);
    wait(|| core.participation(&group.id).unwrap()[0].status == "cancelled");
    assert_eq!(count(&db(root.path()), "SELECT COUNT(*) FROM messages"), 1);
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM persona_events WHERE kind='received'"
        ),
        1
    );
}

#[test]
fn separate_rooms_preserve_old_cutoff_and_actual_newer_accepted_prefix_and_state() {
    for api in ["chat_completions", "responses"] {
        let (root, core, a) = isolated();
        let server = Server::handling(move |body| {
            let input = if api == "responses" {
                body["input"][1]["content"][0]["text"].as_str().unwrap()
            } else {
                body["messages"][1]["content"].as_str().unwrap()
            };
            let view: Value = serde_json::from_str(input).unwrap();
            if view.get("latest_user_message").is_some() {
                return Reply::json(answer(api, "{\"whole_group_question\":false}"));
            }
            let older = view["recent_group_transcript"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["text"]
                == "OLDER_ROOM_TRIGGER";
            Reply::json(answer(
                api,
                if older {
                    "{\"respond\":true}"
                } else {
                    "{\"respond\":false}"
                },
            ))
            .delayed(if older { 900 } else { 0 })
        });
        configure(&core, None, &server.url, api, "");
        let chat=live_support::Server::new(vec![live_support::stream(live_support::candidate(&json!({"reply":"NEWER_ACCEPTED_PRIVATE_KNOWLEDGE","state":{"affinity_user":37,"mood":"alert"}}).to_string())),live_support::stream(live_support::success("Older room generation after newer private context"))]);
        let mut settings = core.settings(None).unwrap();
        settings.chat.enabled = true;
        settings.chat.base_url = chat.url.clone();
        settings.chat.model = "synthetic-chat".into();
        settings.chat.api = "chat_completions".into();
        core.save_settings(
            None,
            &pebble_core::SettingsUpdate {
                chat: settings.chat,
                credential: pebble_core::CredentialChange::Clear,
                context_window_tokens: settings.context_window_tokens,
                max_output_tokens: settings.max_output_tokens,
                persona: None,
            },
        )
        .unwrap();
        let old_room = core
            .create_group("Older room", std::slice::from_ref(&a))
            .unwrap();
        let other_room = core
            .create_group("Other room", std::slice::from_ref(&a))
            .unwrap();
        let (old, old_rx) = submit(&core, &old_room.id, "OLDER_ROOM_TRIGGER", &[]);
        server
            .requests
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let (other, other_rx) = submit(&core, &other_room.id, "OTHER_ROOM_NEW_OBSERVATION", &[]);
        finish(&other, &other_rx);
        let (private, rx) = start(&core, &a, "NEWER_PRIVATE_INPUT");
        let completed = terminal(&rx);
        assert_eq!(
            completed.last().unwrap().kind,
            ChatEventKind::Completed,
            "{:?}",
            completed.last().unwrap()
        );
        let prefix = core.model_request(&private.id).unwrap();
        finish(&old, &old_rx);
        let selected = core.participation(&old_room.id).unwrap().remove(0);
        assert_eq!(selected.status, "selected");
        let request = core
            .model_request(selected.call_id.as_ref().unwrap())
            .unwrap();
        assert_eq!(request.conversation_id, old_room.id);
        assert_eq!(request.session_id, prefix.session_id);
        assert_eq!(request.state_before.unwrap()["affinity_user"], 37);
        let native = group_support::native(
            &core
                .model_request(selected.call_id.as_ref().unwrap())
                .unwrap(),
        );
        assert!(native.contains("NEWER_ACCEPTED_PRIVATE_KNOWLEDGE"));
        let latest = group_support::envelope(
            &core
                .model_request(selected.call_id.as_ref().unwrap())
                .unwrap(),
        )
        .to_string();
        assert!(native.contains("OLDER_ROOM_TRIGGER"));
        assert!(latest.contains(&old.message.id));
        assert!(!latest.contains("OTHER_ROOM_NEW_OBSERVATION"));
        let storage = db(root.path());
        let private_event: i64 = storage
            .query_row(
                "SELECT sequence FROM persona_events WHERE message_id=?1 AND kind='received'",
                [&private.messages[0].id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(request.event_cutoff < private_event || native.contains("NEWER_PRIVATE_INPUT"));
        let cutoff: i64 = storage
            .query_row(
                "SELECT event_cutoff FROM participation_decisions WHERE id=?1",
                [&selected.id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(cutoff < private_event);
        assert_eq!(count(&storage, "SELECT COUNT(*) FROM model_calls"), 2);
    }
}

#[test]
fn dynamic_admission_listener_can_cancel_synchronously_without_deadlock() {
    use std::sync::{Arc, Mutex, mpsc};
    struct CancellingListener {
        group: Arc<Mutex<Option<pebble_core::GroupTask>>>,
        tx: mpsc::Sender<pebble_core::ChatEvent>,
    }
    impl pebble_core::ChatListener for CancellingListener {
        fn on_event(&self, event: pebble_core::ChatEvent) -> Result<(), String> {
            if event.kind == ChatEventKind::Admitted {
                self.group.lock().unwrap().as_ref().unwrap().cancel();
            }
            self.tx.send(event).map_err(|e| e.to_string())
        }
    }
    let (_root, core, a) = isolated();
    let server = Server::new(vec![
        Reply::json(answer("chat_completions", "{\"respond\":true}")).delayed(100),
    ]);
    configure(&core, None, &server.url, "chat_completions", "");
    let room = core
        .create_group("Reentrant cancellation", std::slice::from_ref(&a))
        .unwrap();
    let holder = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel();
    let mut post = core
        .send_room_message(
            &room.id,
            "A callback can stop this reply",
            &[],
            Arc::new(CancellingListener {
                group: holder.clone(),
                tx,
            }),
        )
        .unwrap();
    *holder.lock().unwrap() = post.group.take();
    let (mut cancelled, mut settled) = (false, false);
    while !cancelled || !settled {
        let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        cancelled |= event.kind == ChatEventKind::Cancelled;
        settled |= event.kind == ChatEventKind::GroupSettled;
        assert_ne!(event.kind, ChatEventKind::Completed);
    }
    assert_eq!(
        core.room_messages(&room.id).unwrap().last().unwrap().status,
        MessageStatus::Cancelled
    );
}
