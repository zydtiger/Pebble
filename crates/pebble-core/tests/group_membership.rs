mod classifier_support;
mod group_support;
mod live_support;
mod responses_support;
use classifier_support::{Reply, Server, answer, configure, finish, submit};
use group_support::{count, db, native, task};
use pebble_core::{ChatEventKind, CredentialChange, Error, MessageStatus, SettingsUpdate};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
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
fn generated(api: &str, text: &str, state: i64) -> Reply {
    let text = json!({"reply":text,"state":{"affinity_user":state,"mood":"calm"}}).to_string();
    Reply::stream(if api == "responses" {
        responses_support::candidate(&text)
    } else {
        live_support::candidate(&text)
    })
}

#[test]
fn joins_use_publication_boundary_and_rejoin_preserves_history_without_absent_backfill_both_apis() {
    for api in ["chat_completions", "responses"] {
        let (root, core, a) = live_support::isolated();
        let b = core.create_persona("Kai").unwrap().id;
        let server_a = Server::new(vec![
            generated(api, "PUBLISHED_AFTER_JOIN", 10).delayed(350),
            generated(api, "ABSENT_REPLY", 20),
        ]);
        let server_b = Server::handling(move |_| generated(api, "B_NATIVE_KEEP", 47));
        connection(&core, &a, &server_a.url, api);
        connection(&core, &b, &server_b.url, api);
        let (private, rx) = live_support::start(&core, &b, "B_PRIVATE_BEFORE_JOIN");
        live_support::terminal(&rx);
        let private_prefix = core.model_request(&private.id).unwrap();
        let room = core
            .create_group("Membership publication", std::slice::from_ref(&a))
            .unwrap();
        let (old, rx) = submit(
            &core,
            &room.id,
            "BEFORE_JOIN_ROOT",
            std::slice::from_ref(&a),
        );
        server_a
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let joined = core.add_group_member(&room.id, &b).unwrap();
        let interval = joined
            .members
            .iter()
            .find(|m| m.persona_id == b)
            .unwrap()
            .membership_id
            .clone();
        assert_eq!(
            core.add_group_member(&room.id, &b).unwrap().members.len(),
            2
        );
        finish(&old, &rx);
        assert_eq!(
            core.participation(&room.id).unwrap().len(),
            1,
            "New member must wait for the next user round"
        );
        let (post, rx) = submit(&core, &room.id, "OBSERVED_WHILE_PRESENT", &[]);
        finish(&post, &rx);
        let removed = core.remove_group_member(&room.id, &b).unwrap();
        assert_eq!(removed.former_member_ids, vec![b.clone()]);
        core.remove_group_member(&room.id, &b).unwrap();
        let (absent, rx) = submit(&core, &room.id, "ABSENT_ROOT", std::slice::from_ref(&a));
        finish(&absent, &rx);
        let rejoined = core.add_group_member(&room.id, &b).unwrap();
        assert_ne!(
            rejoined
                .members
                .iter()
                .find(|m| m.persona_id == b)
                .unwrap()
                .membership_id,
            interval
        );
        assert!(rejoined.former_member_ids.is_empty());
        let (post, rx) = submit(
            &core,
            &room.id,
            "AFTER_REJOIN_ROOT",
            std::slice::from_ref(&b),
        );
        let events = finish(&post, &rx);
        let call = events
            .iter()
            .find(|e| e.kind == ChatEventKind::Admitted)
            .unwrap();
        let request = core.model_request(&call.run_id).unwrap();
        for item in &private_prefix.items {
            assert!(
                request
                    .items
                    .iter()
                    .any(|next| next.id == item.id && next.payload == item.payload)
            );
        }
        let history = native(&request);
        for included in [
            "B_PRIVATE_BEFORE_JOIN",
            "B_NATIVE_KEEP",
            "PUBLISHED_AFTER_JOIN",
            "OBSERVED_WHILE_PRESENT",
            "AFTER_REJOIN_ROOT",
        ] {
            assert!(history.contains(included), "{included}");
        }
        for excluded in ["BEFORE_JOIN_ROOT", "ABSENT_ROOT", "ABSENT_REPLY"] {
            assert!(!history.contains(excluded), "{excluded}");
        }
        assert_eq!(request.state_before.unwrap()["affinity_user"], 47);
        let ids = core
            .room_messages(&room.id)
            .unwrap()
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>();
        core.rename_group(&room.id, "Same room, new name").unwrap();
        assert_eq!(
            core.room_messages(&room.id)
                .unwrap()
                .iter()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        let db = db(root.path());
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM conversation_memberships WHERE left_publication IS NOT NULL"
            ),
            1
        );
        assert!(
            !db.prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
        core.remove_group_member(&room.id, &a).unwrap();
        core.remove_group_member(&room.id, &b).unwrap();
        assert!(
            core.workspace()
                .unwrap()
                .rooms
                .iter()
                .find(|r| r.id == room.id)
                .unwrap()
                .members
                .is_empty()
        );
        assert!(core.add_group_member(&room.id, &a).is_ok());
    }
}

#[test]
fn removal_cancels_old_interval_work_only_and_rejoin_never_revives_it_both_apis() {
    for api in ["chat_completions", "responses"] {
        for phase in ["classifying", "queued", "generating"] {
            let (root, core, a) = live_support::isolated();
            let classifier = Server::handling(move |_| {
                Reply::json(answer(api, "{\"respond\":true}")).delayed(650)
            });
            if phase == "classifying" {
                configure(&core, None, &classifier.url, api, "");
            }
            let calls = AtomicUsize::new(0);
            let generation = Server::handling(move |_| {
                let index = calls.fetch_add(1, Ordering::SeqCst);
                generated(api, &format!("NATIVE_ACCEPTED_{index}"), 30 + index as i64).delayed(
                    if phase != "classifying" && index == 0 {
                        650
                    } else {
                        0
                    },
                )
            });
            connection(&core, &a, &generation.url, api);
            let room = core
                .create_group("Removed room", std::slice::from_ref(&a))
                .unwrap();
            let other = core
                .create_group("Unrelated group", std::slice::from_ref(&a))
                .unwrap();
            let interval = room.members[0].membership_id.clone();
            let private = if phase == "queued" {
                let pair = live_support::start(&core, &a, "PRIVATE_CONTINUES");
                generation
                    .requests
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
                Some(pair)
            } else {
                None
            };
            let picked = if phase == "classifying" {
                vec![]
            } else {
                vec![a.clone()]
            };
            let (old, rx) = submit(&core, &room.id, "OLD_INTERVAL_ROOT", &picked);
            if phase == "classifying" {
                classifier
                    .requests
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
            } else {
                let _task = task(&old, 0);
                if phase == "generating" {
                    generation
                        .requests
                        .recv_timeout(Duration::from_secs(3))
                        .unwrap();
                }
            }
            core.remove_group_member(&room.id, &a).unwrap();
            let new = core.add_group_member(&room.id, &a).unwrap();
            assert_ne!(new.members[0].membership_id, interval);
            let private =
                private.unwrap_or_else(|| live_support::start(&core, &a, "PRIVATE_CONTINUES"));
            let (other_post, other_rx) = submit(
                &core,
                &other.id,
                "OTHER_GROUP_CONTINUES",
                std::slice::from_ref(&a),
            );
            finish(&old, &rx);
            live_support::terminal(&private.1);
            finish(&other_post, &other_rx);
            let cancelled = core.participation(&room.id).unwrap().remove(0);
            assert_eq!(cancelled.status, "cancelled");
            assert_eq!(cancelled.slot_status, "cancelled");
            let messages = core.room_messages(&room.id).unwrap();
            if phase != "classifying" {
                let reply = messages.last().unwrap();
                assert_eq!(reply.status, MessageStatus::Cancelled);
                assert!(!reply.can_retry);
                assert!(matches!(
                    core.retry_message(&a, &reply.id, group_support::listener()),
                    Err(Error::RetryUnavailable(_))
                ));
            }
            assert_eq!(
                core.messages(&a).unwrap().last().unwrap().status,
                MessageStatus::Complete
            );
            assert_eq!(
                core.room_messages(&other.id)
                    .unwrap()
                    .last()
                    .unwrap()
                    .status,
                MessageStatus::Complete
            );
            let state = core.response_settings(&a).unwrap().current_state_json;
            drop(classifier);
            assert_eq!(core.participation(&room.id).unwrap().remove(0), cancelled);
            assert_eq!(
                core.response_settings(&a).unwrap().current_state_json,
                state
            );
            let db = db(root.path());
            assert_eq!(
                count(&db, "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"),
                2
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM model_calls WHERE status='completed'"
                ),
                2
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM group_reply_slots WHERE status IN ('pending','admitted')"
                ),
                0
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM participation_decisions WHERE status='pending'"
                ),
                0
            );
            assert_eq!(
                db.query_row(
                    "SELECT COUNT(*) FROM persona_events WHERE kind='received' AND message_id=?1",
                    [&old.message.id],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                1,
                "Cancellation must retain observations"
            );
            core.fresh_context(&a).unwrap();
            drop(generation);
        }
    }
}

#[test]
fn rejoin_cannot_retry_an_old_interval_even_without_a_newer_persona_call_both_apis() {
    for api in ["chat_completions", "responses"] {
        let (root, core, a) = live_support::isolated();
        let server = Server::handling(move |_| generated(api, "MUST_NOT_PUBLISH", 99).delayed(300));
        connection(&core, &a, &server.url, api);
        let room = core
            .create_group("Retry interval", std::slice::from_ref(&a))
            .unwrap();
        let (post, rx) = submit(
            &core,
            &room.id,
            "KEEP_OBSERVATION",
            std::slice::from_ref(&a),
        );
        server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        task(&post, 0).cancel();
        finish(&post, &rx);
        let reply = core.room_messages(&room.id).unwrap().pop().unwrap();
        assert!(
            reply.can_retry,
            "Current interval's latest stopped call is eligible"
        );
        let state = core.response_settings(&a).unwrap().current_state_json;
        core.remove_group_member(&room.id, &a).unwrap();
        core.add_group_member(&room.id, &a).unwrap();
        assert!(
            !core
                .room_messages(&room.id)
                .unwrap()
                .last()
                .unwrap()
                .can_retry
        );
        assert!(matches!(
            core.retry_message(&a, &reply.id, group_support::listener()),
            Err(Error::RetryUnavailable(_))
        ));
        drop(server);
        let db = db(root.path());
        assert_eq!(count(&db, "SELECT COUNT(*) FROM model_calls"), 1);
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM model_items WHERE kind='output'"),
            0
        );
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"),
            0
        );
        assert_eq!(
            core.response_settings(&a).unwrap().current_state_json,
            state
        );
    }
}
