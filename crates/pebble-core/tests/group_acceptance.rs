mod group_support;
mod live_support;
use group_support::*;
use live_support::{Plan, Server, configure, isolated, start, stream, terminal};
use pebble_core::{ChatEventKind, CredentialChange, Error, MessageStatus, SettingsUpdate};
use std::time::Duration;

#[test]
fn mentions_are_current_stable_deduplicated_and_never_guess_duplicate_names() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Mira").unwrap();
    let c = core.create_persona("Other").unwrap();
    assert!(core.create_group("empty", &[]).is_err());
    assert!(core.create_group(" ", std::slice::from_ref(&a)).is_err());
    assert!(core.create_group("bad", &["missing".into()]).is_err());
    let group = core
        .create_group("One or more", &[a.clone(), b.id.clone(), a.clone()])
        .unwrap();
    assert_eq!(group.members.len(), 2);
    for (text, picks) in [
        ("@Mira ambiguous", vec![]),
        ("@Other absent", vec![]),
        ("unknown selected", vec![c.id.clone()]),
    ] {
        assert!(
            core.send_room_message(&group.id, text, &picks, listener())
                .is_err()
        );
    }
    assert!(core.room_messages(&group.id).unwrap().is_empty());
    let (picked, rx) = submit(
        &core,
        &group.id,
        "Body can be edited independently",
        &[b.id.clone(), b.id.clone()],
    );
    assert!(
        picked.tasks.is_empty(),
        "Group calls are admitted only when ready"
    );
    assert_eq!(
        picked.message.content,
        "@Mira\nBody can be edited independently"
    );
    finish(&rx, 1);
    let db = db(root.path());
    let saved: String = db
        .query_row(
            "SELECT persona_id FROM message_mentions WHERE message_id=?1",
            [&picked.message.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(saved, b.id);
    let settings = core.settings(Some(&b.id)).unwrap();
    let mut persona = settings.persona.unwrap();
    persona.display_name = "Renamed Kai".into();
    core.save_settings(
        Some(&b.id),
        &SettingsUpdate {
            chat: settings.chat,
            credential: CredentialChange::Keep,
            context_window_tokens: settings.context_window_tokens,
            max_output_tokens: settings.max_output_tokens,
            persona: Some(persona),
        },
    )
    .unwrap();
    let (typed, rx) = submit(&core, &group.id, "@Renamed Kai, hello @Renamed Kai!", &[]);
    assert!(
        typed.tasks.is_empty(),
        "Group calls are admitted only when ready"
    );
    finish(&rx, 1);
    assert_eq!(
        db.query_row(
            "SELECT persona_id FROM message_mentions WHERE message_id=?1",
            [&picked.message.id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        saved
    );
    let renamed = core.rename_group(&group.id, "New mutable name").unwrap();
    assert_eq!(renamed.id, group.id);
    assert_eq!(core.room_messages(&group.id).unwrap().len(), 4);
    let (no_call, _) = submit(
        &core,
        &group.id,
        "email user@example.com is ordinary text",
        &[],
    );
    assert!(no_call.tasks.is_empty());
    let (only_mention, rx) = submit(&core, &group.id, "", &[a]);
    assert!(
        only_mention.tasks.is_empty(),
        "Group calls are admitted only when ready"
    );
    finish(&rx, 1);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM conversation_memberships WHERE joined_publication=0 AND left_publication IS NULL"
        ),
        2
    );
}

#[test]
fn shared_persona_queue_keeps_destinations_state_and_cancelled_public_observations() {
    let body = |reply: &str, n: i64| {
        live_support::candidate(
            &serde_json::json!({"reply":reply,"state":{"affinity_user":n,"mood":"calm"}})
                .to_string(),
        )
    };
    let server = Server::new(vec![
        Plan {
            status: 200,
            body: body("first private output", 21),
            delay_headers: false,
            delay_tail: true,
        },
        stream(body("second room output", 22)),
        stream(body("private continuation", 23)),
    ]);
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap();
    let first_room = core
        .create_group("First", &[a.clone(), b.id.clone()])
        .unwrap();
    let second_room = core
        .create_group("Second", std::slice::from_ref(&a))
        .unwrap();
    configure(
        root.path(),
        &server.url,
        "synthetic",
        "chat_completions",
        "",
    );
    let (_private, rx) = start(&core, &a, "private predecessor");
    server
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let (cancelled, cancel_rx) = submit(
        &core,
        &first_room.id,
        "OBSERVE_EVEN_CANCELLED",
        std::slice::from_ref(&a),
    );
    let (second, second_rx) = submit(
        &core,
        &second_room.id,
        "SECOND_ROOM_INPUT",
        std::slice::from_ref(&a),
    );
    let _second_ready = task(&second, 0);
    let (private, private_rx) = start(&core, &a, "later private input");
    assert_eq!(
        core.room_messages(&second_room.id).unwrap()[1].status,
        MessageStatus::Queued
    );
    assert!(matches!(core.fresh_context(&a), Err(Error::Busy)));
    let settings = core.settings(Some(&a)).unwrap();
    assert!(matches!(
        core.save_settings(
            Some(&a),
            &SettingsUpdate {
                chat: settings.chat,
                credential: CredentialChange::Keep,
                context_window_tokens: settings.context_window_tokens,
                max_output_tokens: settings.max_output_tokens,
                persona: settings.persona
            }
        ),
        Err(Error::Busy)
    ));
    task(&cancelled, 0).cancel();
    assert_eq!(
        finish(&cancel_rx, 1).last().unwrap().kind,
        ChatEventKind::Cancelled
    );
    assert!(!core.room_messages(&first_room.id).unwrap()[1].can_retry);
    terminal(&rx);
    assert_eq!(
        finish(&second_rx, 1).last().unwrap().kind,
        ChatEventKind::Completed
    );
    terminal(&private_rx);
    let request = core.model_request(&task(&second, 0).id).unwrap();
    let input = envelope(&request);
    assert_eq!(input["current_state"]["affinity_user"], 21);
    assert_eq!(input["reply_request"]["conversation_id"], second_room.id);
    let events = input["events"].as_array().unwrap();
    assert!(
        events
            .iter()
            .any(|e| e["text"] == "@Mira\nOBSERVE_EVEN_CANCELLED")
    );
    assert!(events.iter().all(|e| e["text"] != "later private input"));
    assert!(native(&request).contains("first private output"));
    let request = core.model_request(&private.id).unwrap();
    assert_eq!(envelope(&request)["current_state"]["affinity_user"], 22);
    assert_eq!(
        envelope(&request)["reply_request"]["conversation_id"],
        core.workspace()
            .unwrap()
            .personas
            .iter()
            .find(|p| p.id == a)
            .unwrap()
            .conversation_id
    );
    assert_eq!(core.room_messages(&first_room.id).unwrap().len(), 2);
    assert_eq!(
        core.room_messages(&second_room.id).unwrap()[1].content,
        "second room output"
    );
    let db = db(root.path());
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM model_calls WHERE persona_id IS NULL OR persona_sequence IS NULL"
        ),
        0
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM model_calls WHERE status='completed'"
        ),
        3
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM persona_events WHERE kind='received' AND persona_id=(SELECT id FROM personas WHERE display_name='Kai')"
        ),
        1
    );
}
