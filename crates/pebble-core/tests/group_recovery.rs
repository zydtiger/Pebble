mod group_support;
mod live_support;
use group_support::*;
use live_support::{isolated, reopen, retry, start, terminal};
use pebble_core::{ChatEventKind, Error, MessageStatus};
use std::time::Duration;

#[test]
fn acceptance_and_publication_roll_back_all_group_effects_and_retry_keeps_destination() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap();
    let group = core
        .create_group("Group", &[a.clone(), b.id.clone()])
        .unwrap();
    let db = db(root.path());
    db.execute_batch("CREATE TRIGGER reject_member BEFORE INSERT ON persona_events WHEN NEW.persona_id=(SELECT id FROM personas WHERE display_name='Kai') BEGIN SELECT RAISE(ABORT,'receipt rollback'); END;").unwrap();
    assert!(
        core.send_room_message(
            &group.id,
            "canonical once",
            std::slice::from_ref(&a),
            listener()
        )
        .is_err()
    );
    for table in [
        "messages",
        "persona_events",
        "message_mentions",
        "model_calls",
    ] {
        assert_eq!(count(&db, &format!("SELECT COUNT(*) FROM {table}")), 0);
    }
    assert_eq!(
        count(&db, "SELECT SUM(publication_sequence) FROM conversations"),
        0
    );
    db.execute_batch("DROP TRIGGER reject_member; CREATE TRIGGER reject_publication BEFORE INSERT ON persona_events WHEN NEW.kind='received' AND NEW.message_id IN (SELECT id FROM messages WHERE sender_kind='persona') BEGIN SELECT RAISE(ABORT,'publication rollback'); END;").unwrap();
    let before = core.response_settings(&a).unwrap();
    let (failed, rx) = submit(
        &core,
        &group.id,
        "publication must be atomic",
        std::slice::from_ref(&a),
    );
    assert_eq!(finish(&rx, 1).last().unwrap().kind, ChatEventKind::Failed);
    let failed_reply = core.room_messages(&group.id).unwrap()[1].clone();
    assert!(failed_reply.can_retry);
    assert_eq!(failed_reply.status, MessageStatus::Failed);
    assert_eq!(
        core.response_settings(&a).unwrap().current_state_json,
        before.current_state_json
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM model_items WHERE kind='output'"),
        0
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"),
        0
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM messages WHERE publication_sequence IS NOT NULL"
        ),
        1
    );
    // Another speaker's newer call in this same room does not supersede A's retry.
    db.execute_batch("DROP TRIGGER reject_publication;")
        .unwrap();
    let (_b, rx) = submit(&core, &group.id, "Kai speaks later", &[b.id]);
    finish(&rx, 1);
    assert!(core.room_messages(&group.id).unwrap()[1].can_retry);
    let (retried, rx) = retry(&core, &a, &failed_reply.id);
    terminal(&rx);
    assert_eq!(
        core.model_request(&retried.id).unwrap().conversation_id,
        group.id
    );
    let messages = core.room_messages(&group.id).unwrap();
    assert_eq!(
        messages.iter().filter(|m| m.sender_kind == "user").count(),
        2
    );
    assert_eq!(messages.last().unwrap().status, MessageStatus::Complete);
    assert_eq!(messages[0].id, failed.message.id);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM message_mentions"), 2);
    let (old, rx) = submit(&core, &group.id, "Stop this", std::slice::from_ref(&a));
    task(&old, 0).cancel();
    finish(&rx, 1);
    let stopped = core
        .room_messages(&group.id)
        .unwrap()
        .last()
        .unwrap()
        .id
        .clone();
    assert!(
        core.room_messages(&group.id)
            .unwrap()
            .last()
            .unwrap()
            .can_retry
    );
    let (_new, rx) = start(&core, &a, "newer call in private room");
    terminal(&rx);
    assert!(
        !core
            .room_messages(&group.id)
            .unwrap()
            .last()
            .unwrap()
            .can_retry
    );
    assert!(matches!(
        core.retry_message(&a, &stopped, listener()),
        Err(Error::RetryUnavailable(_))
    ));
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}

#[test]
fn restart_interrupts_group_calls_without_losing_observations_or_accepting_outputs() {
    let root = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "group_crash_fixture", "--nocapture"])
        .env("PEBBLE_GROUP_CRASH", root.path())
        .status()
        .unwrap();
    assert!(status.success());
    let core = reopen(root.path());
    let workspace = core.workspace().unwrap();
    let a = &workspace.personas[0];
    let group = workspace.rooms.iter().find(|r| r.kind == "group").unwrap();
    let messages = core.room_messages(&group.id).unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(
        count(
            &db(root.path()),
            "SELECT COUNT(*) FROM group_reply_slots WHERE status='interrupted'"
        ),
        2
    );
    assert!(
        messages
            .iter()
            .filter(|m| m.sender_kind == "persona")
            .all(|m| m.status == MessageStatus::Interrupted && m.content.is_empty())
    );
    let db = db(root.path());
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM model_items WHERE kind='output'"),
        0
    );
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"),
        0
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM messages WHERE publication_sequence IS NOT NULL"
        ),
        2
    );
    let (next, rx) = start(&core, &a.id, "after restart");
    terminal(&rx);
    let request = core.model_request(&next.id).unwrap();
    let history = native(&request);
    assert!(history.contains("before crash active"));
    assert!(history.contains("before crash queued"));
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM model_item_events x JOIN persona_events e ON e.id=x.event_id JOIN model_items i ON i.id=x.item_id WHERE e.kind='received' AND i.session_id=(SELECT id FROM model_sessions WHERE status='active')"
        ),
        3
    );
}
#[test]
fn group_crash_fixture() {
    let Some(root) = std::env::var_os("PEBBLE_GROUP_CRASH") else {
        return;
    };
    let core = pebble_core::Core::open(root).unwrap();
    let a = core.workspace().unwrap().personas[0].id.clone();
    let group = core
        .create_group("Restart group", std::slice::from_ref(&a))
        .unwrap();
    let (_active, rx) = submit(
        &core,
        &group.id,
        "before crash active",
        std::slice::from_ref(&a),
    );
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (_queued, _rx) = submit(&core, &group.id, "before crash queued", &[a]);
    std::process::exit(0);
}
