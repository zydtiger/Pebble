mod group_support;
mod live_support;
mod responses_support;
use group_support::*;
use live_support::*;
use pebble_core::{ChatEventKind, MessageStatus};
use serde_json::json;

#[test]
fn both_protocols_share_persona_history_state_but_only_public_observations_with_members() {
    for api in ["chat_completions", "responses"] {
        let body = |reply: &str, affinity: i64| {
            let text =
                json!({"reply":reply,"state":{"affinity_user":affinity,"mood":"calm"}}).to_string();
            if api == "responses" {
                responses_support::candidate(&text)
            } else {
                candidate(&text)
            }
        };
        let server = Server::new(vec![
            stream(body("A_PRIVATE_REPLY", 31)),
            stream(body("A_PUBLIC_REPLY", 32)),
            stream(body("B_PRIVATE_REPLY", 41)),
            stream(body("A_CONTINUATION", 33)),
            stream(body("C_PRIVATE_REPLY", 51)),
        ]);
        let (root, core, a) = isolated();
        let b = core.create_persona("Kai").unwrap();
        let c = core.create_persona("Outsider").unwrap();
        configure(root.path(), &server.url, "synthetic", api, "");
        let (private, rx) = start(&core, &a, "A_PRIVATE_SECRET");
        terminal(&rx);
        let old = core.model_request(&private.id).unwrap();
        let room = core
            .create_group("Shared room", &[a.clone(), b.id.clone()])
            .unwrap();
        let (silent, rx) = submit(&core, &room.id, "PUBLIC_LISTEN_ONLY", &[]);
        assert!(silent.tasks.is_empty());
        finish(&rx, 0);
        let (group, rx) = submit(&core, &room.id, "@Mira PUBLIC_GROUP_INPUT", &[]);
        assert!(
            group.tasks.is_empty(),
            "Group calls are admitted only when ready"
        );
        assert_eq!(
            finish(&rx, 1).last().unwrap().kind,
            ChatEventKind::Completed
        );
        let request = core.model_request(&task(&group, 0).id).unwrap();
        assert_eq!(request.session_id, old.session_id);
        for (a, b) in old.items.iter().zip(&request.items) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.payload, b.payload);
        }
        let input = envelope(&request);
        assert_eq!(input["current_state"]["affinity_user"], 31);
        assert_eq!(input["reply_request"]["conversation_id"], room.id);
        assert_eq!(input["reply_request"]["room"]["kind"], "group");
        assert_eq!(input["events"].as_array().unwrap().len(), 2);
        let (b_private, rx) = start(&core, &b.id, "B_PRIVATE_INPUT");
        terminal(&rx);
        let b_request = core.model_request(&b_private.id).unwrap();
        let input = envelope(&b_request);
        let observations = input["events"].as_array().unwrap();
        assert_eq!(observations.len(), 4);
        assert_eq!(
            observations
                .iter()
                .filter(|e| e["text"] == "A_PUBLIC_REPLY")
                .count(),
            1
        );
        assert!(
            observations
                .iter()
                .any(|e| e["speaker"]["id"] == a && e["speaker"]["name"] == "Mira")
        );
        let b_native = native(&b_request);
        assert!(!b_native.contains("A_PRIVATE"));
        assert!(!b_native.contains("private-test-reasoning"));
        assert!(!b_native.contains("sealed-test-state"));
        assert_eq!(input["current_state"]["affinity_user"], 20);
        let (continuation, rx) = start(&core, &a, "A_NEXT_PRIVATE");
        terminal(&rx);
        let next = core.model_request(&continuation.id).unwrap();
        assert_eq!(next.session_id, old.session_id);
        assert_eq!(envelope(&next)["current_state"]["affinity_user"], 32);
        let replay = native(&next);
        assert!(replay.contains("A_PRIVATE_SECRET"));
        assert!(replay.contains("A_PUBLIC_REPLY"));
        assert!(!replay.contains("B_PRIVATE_INPUT"));
        assert!(replay.contains(if api == "responses" {
            "sealed-test-state"
        } else {
            "private-test-reasoning"
        }));
        let (outsider, rx) = start(&core, &c.id, "C_ONLY");
        terminal(&rx);
        let outsider = native(&core.model_request(&outsider.id).unwrap());
        assert!(!outsider.contains("PUBLIC_"));
        assert!(!outsider.contains("A_PRIVATE"));
        let messages = core.room_messages(&room.id).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2].sender_id, a);
        assert_eq!(messages[2].status, MessageStatus::Complete);
        assert_eq!(messages[2].content, "A_PUBLIC_REPLY");
        let db = db(root.path());
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM persona_events WHERE kind='received' AND message_id IN (SELECT id FROM messages WHERE sender_kind='persona' AND content='A_PUBLIC_REPLY')"
            ),
            1
        );
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM persona_events e JOIN messages m ON m.id=e.message_id WHERE e.persona_id=m.sender_id AND e.kind='received'"
            ),
            0
        );
        assert!(
            !db.prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
        drop((private, silent, group, b_private, continuation));
        drop(core);
        let reopened = reopen(root.path());
        assert_eq!(reopened.room_messages(&room.id).unwrap(), messages);
        assert_eq!(
            reopened
                .workspace()
                .unwrap()
                .rooms
                .iter()
                .find(|r| r.id == room.id)
                .unwrap()
                .members
                .len(),
            2
        );
    }
}

#[test]
fn silent_member_later_mentioned_in_group_keeps_own_native_outputs_without_reinjection_both_apis() {
    for api in ["chat_completions", "responses"] {
        let body = |reply: &str| {
            let text =
                json!({"reply":reply,"state":{"affinity_user":42,"mood":"calm"}}).to_string();
            if api == "responses" {
                responses_support::candidate(&text)
            } else {
                candidate(&text)
            }
        };
        let server = Server::new(vec![
            stream(body("KAI_PRIVATE_NATIVE")),
            stream(body("MIRA_PUBLIC_OBSERVED")),
            stream(body("KAI_PUBLIC_NATIVE")),
            stream(body("KAI_CONTINUED")),
        ]);
        let (root, core, a) = isolated();
        let b = core.create_persona("Kai").unwrap().id;
        configure(root.path(), &server.url, "synthetic", api, "");
        let (private, rx) = start(&core, &b, "Kai's earlier private context");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let prefix = core.model_request(&private.id).unwrap();
        let group = core
            .create_group("Silent to mentioned", &[a.clone(), b.clone()])
            .unwrap();
        let (silent, rx) = submit(&core, &group.id, "PUBLIC_SILENT_ROOT", &[]);
        finish(&rx, 0);
        assert!(silent.tasks.is_empty());
        let (mira, rx) = submit(&core, &group.id, "@Mira PUBLIC_MIRA_ROOT", &[]);
        finish(&rx, 1);
        let (kai, rx) = submit(&core, &group.id, "@Kai PUBLIC_KAI_REQUEST", &[]);
        assert_eq!(
            finish(&rx, 1).last().unwrap().kind,
            ChatEventKind::Completed
        );
        let request = core.model_request(&task(&kai, 0).id).unwrap();
        assert_eq!(request.session_id, prefix.session_id);
        for (old, new) in prefix.items.iter().zip(&request.items) {
            assert_eq!(old.id, new.id);
            assert_eq!(old.payload, new.payload);
        }
        let input = envelope(&request);
        let events = input["events"].as_array().unwrap();
        for expected in [
            "PUBLIC_SILENT_ROOT",
            "@Mira PUBLIC_MIRA_ROOT",
            "MIRA_PUBLIC_OBSERVED",
            "@Kai PUBLIC_KAI_REQUEST",
        ] {
            assert_eq!(
                events.iter().filter(|e| e["text"] == expected).count(),
                1,
                "{expected}"
            );
        }
        assert!(events.iter().all(|e| e["text"] != "KAI_PRIVATE_NATIVE"));
        assert_eq!(native(&request).matches("KAI_PRIVATE_NATIVE").count(), 1);
        assert_eq!(input["reply_request"]["conversation_id"], group.id);
        assert_eq!(input["current_state"]["affinity_user"], 42);
        let (continued_task, rx) = start(&core, &b, "Continue after group");
        terminal(&rx);
        let continued = core.model_request(&continued_task.id).unwrap();
        assert_eq!(native(&continued).matches("KAI_PUBLIC_NATIVE").count(), 1);
        assert!(
            envelope(&continued)["events"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["text"] != "KAI_PUBLIC_NATIVE")
        );
        assert_eq!(db(root.path()).query_row("SELECT COUNT(*) FROM persona_events e JOIN messages m ON m.id=e.message_id WHERE e.persona_id=?1 AND e.kind='received' AND m.sender_id=?1", [&b], |r| r.get::<_,i64>(0)).unwrap(), 0);
        drop((mira, kai));
    }
}
