mod classifier_support;
mod group_support;
mod live_support;
mod responses_support;
use classifier_support::{finish, submit};
use group_support::{count, db, envelope, native};
use live_support::{Server, isolated, start, stream, terminal};
use pebble_core::{
    ChatEventKind, CredentialChange, Error, ResponseAction, ResponseUpdate, SettingsUpdate,
};
use serde_json::json;
use std::time::Duration;

fn connection(core: &pebble_core::Core, id: &str, url: &str, api: &str) {
    let mut settings = core.settings(Some(id)).unwrap();
    settings.chat.enabled = true;
    settings.chat.context_len = Some(65536);
    settings.chat.max_output_tokens = Some(4096);
    settings.chat.base_url = url.into();
    settings.chat.model = "synthetic".into();
    settings.chat.api = api.into();
    core.save_settings(
        Some(id),
        &SettingsUpdate {
            chat: settings.chat,
            credential: CredentialChange::Clear,
            persona: settings.persona,
        },
    )
    .unwrap();
}
fn candidate(api: &str, reply: &str, affinity: i64) -> String {
    let text = json!({"reply":reply,"state":{"affinity_user":affinity,"mood":"calm"}}).to_string();
    if api == "responses" {
        responses_support::candidate(&text)
    } else {
        live_support::candidate(&text)
    }
}

#[test]
fn ready_only_required_slots_preserve_actual_prefix_state_and_coverage_holes_both_apis() {
    for api in ["chat_completions", "responses"] {
        for with_private in [false, true] {
            let (root, core, a) = isolated();
            let b = core.create_persona("Kai").unwrap().id;
            let mut held = stream(candidate(api, "B_ACCEPTED_PREDECESSOR", 20));
            held.delay_headers = true;
            let server_b = Server::new(vec![held]);
            let mut plans = Vec::new();
            if with_private {
                plans.push(stream(candidate(api, "PRIVATE_P_NATIVE", 47)));
            }
            plans.push(stream(candidate(api, "A_DELAYED_REQUIRED", 47)));
            plans.push(stream(candidate(api, "AFTER_ROOT", 47)));
            let server_a = Server::new(plans);
            connection(&core, &a, &server_a.url, api);
            connection(&core, &b, &server_b.url, api);
            let room = core
                .create_group("Causal room", &[b.clone(), a.clone()])
                .unwrap();
            let (first, events) = submit(
                &core,
                &room.id,
                "U1_ORIGINAL_REQUIRED",
                &[b.clone(), a.clone()],
            );
            server_b
                .requests
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
            assert_eq!(
                count(
                    &db(root.path()),
                    &format!("SELECT COUNT(*) FROM model_calls WHERE persona_id='{a}'")
                ),
                0,
                "A must not reserve future persona work"
            );
            assert!(matches!(core.fresh_context(&a), Err(Error::Busy)));
            let expected = core.response_settings(&a).unwrap();
            assert!(matches!(
                core.save_response_settings(
                    &a,
                    &ResponseUpdate {
                        schema_json: expected.schema_json.clone(),
                        initial_state_json: expected.initial_state_json.clone(),
                        expected,
                        action: ResponseAction::FreshContextResetState
                    }
                ),
                Err(Error::Busy)
            ));
            let (second, second_events) = submit(&core, &room.id, "U2_LATER_OBSERVATION", &[]);
            let private = if with_private {
                let (task, rx) = start(&core, &a, "P_PRIVATE_CALL");
                assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
                Some(core.model_request(&task.id).unwrap())
            } else {
                None
            };
            let events = finish(&first, &events);
            finish(&second, &second_events);
            let calls = events
                .iter()
                .filter(|e| e.kind == ChatEventKind::Admitted)
                .collect::<Vec<_>>();
            assert_eq!(
                calls
                    .iter()
                    .map(|e| e.persona_id.as_str())
                    .collect::<Vec<_>>(),
                vec![b.as_str(), a.as_str()]
            );
            let request = core.model_request(&calls[1].run_id).unwrap();
            let input = envelope(&request);
            assert_eq!(request.conversation_id, room.id);
            assert_eq!(request.trigger_message_id, first.message.id);
            assert!(
                input["events"]
                    .to_string()
                    .contains("B_ACCEPTED_PREDECESSOR")
            );
            assert!(!input["events"].to_string().contains("U2_LATER_OBSERVATION"));
            if let Some(private) = private {
                assert_eq!(request.state_before.as_ref().unwrap()["affinity_user"], 47);
                for earlier in &private.items {
                    assert!(
                        request
                            .items
                            .iter()
                            .any(|item| item.id == earlier.id && item.payload == earlier.payload)
                    );
                }
                assert!(native(&request).contains("PRIVATE_P_NATIVE"));
                assert_eq!(native(&request).matches("U2_LATER_OBSERVATION").count(), 1);
            } else {
                assert!(!native(&request).contains("U2_LATER_OBSERVATION"));
            }
            let (later, rx) = start(&core, &a, "LATER_ELIGIBLE_INPUT");
            assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
            let later = core.model_request(&later.id).unwrap();
            assert_eq!(native(&later).matches("U2_LATER_OBSERVATION").count(), 1);
            assert_eq!(native(&later).matches("B_ACCEPTED_PREDECESSOR").count(), 1);
            assert_eq!(
                count(
                    &db(root.path()),
                    "SELECT COUNT(*) FROM group_rounds WHERE status='completed'"
                ),
                2
            );
        }
    }
}

#[test]
fn room_stop_releases_unadmitted_required_context_guards() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap().id;
    let mut held = stream(live_support::success("Must not publish"));
    held.delay_headers = true;
    let server = Server::new(vec![held]);
    connection(&core, &b, &server.url, "chat_completions");
    let room = core
        .create_group("Stop waiting", &[b.clone(), a.clone()])
        .unwrap();
    let (submission, rx) = submit(&core, &room.id, "Pending required", &[b, a.clone()]);
    server
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let before = core.response_settings(&a).unwrap();
    assert!(matches!(core.fresh_context(&a), Err(Error::Busy)));
    core.stop_room(&room.id).unwrap();
    finish(&submission, &rx);
    core.fresh_context(&a).unwrap();
    assert_ne!(
        core.response_settings(&a).unwrap().session_id,
        before.session_id
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM group_reply_slots WHERE status IN ('pending','admitted')"
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
}

#[test]
fn queued_optional_reassesses_after_actual_private_state_changes_without_accepting_declined_output()
{
    use classifier_support::{Reply, answer, configure};
    let (root, core, a) = isolated();
    let classifier = classifier_support::Server::handling(|request| {
        let view: serde_json::Value =
            serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
        let yes = view["MEMORY_current_state"]["current_state"]["affinity_user"] != 47;
        Reply::json(answer(
            "chat_completions",
            if yes {
                "{\"respond\":true}"
            } else {
                "{\"respond\":false}"
            },
        ))
    });
    configure(&core, None, &classifier.url, "chat_completions", "");
    let private = classifier_support::Server::handling(|_| {
        Reply::stream(candidate("chat_completions", "P_BEFORE_OPTIONAL", 47)).delayed(350)
    });
    connection(&core, &a, &private.url, "chat_completions");
    let (_p, rx) = start(&core, &a, "Private changes participation state");
    private
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let room = core
        .create_group("Queued reassessment", std::slice::from_ref(&a))
        .unwrap();
    let (post, events) = submit(&core, &room.id, "Optional root", &[]);
    classifier_support::wait(|| {
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM group_reply_slots WHERE status='admitted'",
        ) == 1
    });
    terminal(&rx);
    finish(&post, &events);
    assert_eq!(classifier.requests.try_iter().count(), 2);
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM model_calls WHERE status='cancelled'"
        ),
        1
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM model_items WHERE kind='output'"
        ),
        1
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"
        ),
        1
    );
    assert!(private.requests.try_recv().is_err());
    let d = core.participation(&room.id).unwrap().remove(0);
    assert_eq!(d.status, "listening");
    assert_eq!(d.respond, Some(false));
}

#[test]
fn all_room_stop_reaches_old_protected_and_new_round_without_cancelling_shared_private_or_other_room()
 {
    use classifier_support::{Reply, Server};
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap().id;
    let server_a = Server::handling(|_| {
        Reply::stream(candidate("chat_completions", "A_OTHER_DESTINATION", 31)).delayed(400)
    });
    let server_b = Server::handling(|_| {
        Reply::stream(candidate("chat_completions", "B_MUST_NOT_PUBLISH", 20)).delayed(600)
    });
    connection(&core, &a, &server_a.url, "chat_completions");
    connection(&core, &b, &server_b.url, "chat_completions");
    let room = core
        .create_group("Stopped room", &[b.clone(), a.clone()])
        .unwrap();
    let other = core
        .create_group("Other room", std::slice::from_ref(&a))
        .unwrap();
    let (_private, private_rx) = start(&core, &a, "Unrelated private continues");
    server_a
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let (old, oldrx) = submit(&core, &room.id, "Protected old user root", &[b, a.clone()]);
    server_b
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let (new, newrx) = submit(
        &core,
        &room.id,
        "New required root",
        std::slice::from_ref(&a),
    );
    let (elsewhere, otherrx) = submit(
        &core,
        &other.id,
        "Independent other room",
        std::slice::from_ref(&a),
    );
    core.stop_room(&room.id).unwrap();
    finish(&old, &oldrx);
    finish(&new, &newrx);
    assert_eq!(
        terminal(&private_rx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    assert_eq!(
        finish(&elsewhere, &otherrx)
            .iter()
            .filter(|e| e.kind == ChatEventKind::Completed)
            .count(),
        1
    );
    assert_eq!(
        count(
            &db(root.path()),
            &format!(
                "SELECT COUNT(*) FROM group_rounds WHERE conversation_id='{}' AND status='cancelled'",
                room.id
            )
        ),
        2
    );
    assert!(
        core.room_messages(&room.id)
            .unwrap()
            .iter()
            .all(|m| m.sender_kind == "user" || m.status == pebble_core::MessageStatus::Cancelled)
    );
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"
        ),
        2
    );
}
