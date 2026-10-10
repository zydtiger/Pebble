mod live_support;
use live_support::*;
use pebble_core::{ChatEventKind, MessageStatus};
use serde_json::Value;
use std::sync::{Arc, Barrier};
use std::time::Duration;

#[test]
fn queued_followups_freeze_in_execution_order_and_skip_cancelled_inputs() {
    let server = Server::new(vec![
        Plan {
            status: 200,
            body: success("first native reply"),
            delay_headers: false,
            delay_tail: true,
        },
        stream(success("second reply")),
        stream(success("last reply")),
    ]);
    let (root, core, id) = isolated();
    configure(
        root.path(),
        &server.url,
        "queued-model",
        "chat_completions",
        "",
    );
    let (first, first_rx) = start(&core, &id, "first input");
    server
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let (second, second_rx) = start(&core, &id, "second input");
    let (cancelled, cancel_rx) = start(&core, &id, "never consume this queued input");
    let (last, last_rx) = start(&core, &id, "last input");
    assert_eq!(core.messages(&id).unwrap()[3].status, MessageStatus::Queued);
    assert!(core.model_request(&second.id).is_err());
    cancelled.cancel();
    assert_eq!(
        terminal(&cancel_rx).last().unwrap().kind,
        ChatEventKind::Cancelled
    );
    assert_eq!(
        terminal(&first_rx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    assert_eq!(
        terminal(&second_rx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    assert_eq!(
        terminal(&last_rx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    let a = core.model_request(&first.id).unwrap();
    let b = core.model_request(&second.id).unwrap();
    let c = core.model_request(&last.id).unwrap();
    assert_eq!(a.items.len(), 2);
    assert_eq!(b.items.len(), 4);
    assert_eq!(c.items.len(), 6);
    assert!(!String::from_utf8_lossy(&a.items[1].payload).contains("second input"));
    assert_eq!(
        serde_json::from_slice::<Value>(&b.items[2].payload).unwrap()["content"],
        structured("first native reply")
    );
    assert!(
        c.items
            .iter()
            .all(|i| !String::from_utf8_lossy(&i.payload).contains("never consume"))
    );
    for (early, later) in a.items.iter().zip(&b.items) {
        assert_eq!(early.payload, later.payload);
    }
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let frontier: i64 = db
        .query_row(
            "SELECT covered_event_sequence FROM model_sessions",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let max: i64 = db
        .query_row("SELECT MAX(sequence) FROM persona_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(frontier, max);
    let cutoff: i64 = db
        .query_row(
            "SELECT event_cutoff FROM model_calls WHERE id = ?1",
            [&first.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cutoff, 1);
}

#[test]
fn personas_run_concurrently_and_active_cancellation_is_independent() {
    // Two Started callbacks coordinate, proving neither persona waits for the other.
    struct Gated(
        std::sync::mpsc::Sender<pebble_core::ChatEvent>,
        Arc<Barrier>,
    );
    impl pebble_core::ChatListener for Gated {
        fn on_event(&self, event: pebble_core::ChatEvent) -> Result<(), String> {
            if event.kind == ChatEventKind::Started {
                self.1.wait();
            }
            self.0.send(event).map_err(|e| e.to_string())
        }
    }
    let (_root, core, a) = isolated();
    let b = core.create_persona("Kai \"quoted\" 🪨").unwrap();
    let gate = Arc::new(Barrier::new(2));
    let (txa, rxa) = std::sync::mpsc::channel();
    let (txb, rxb) = std::sync::mpsc::channel();
    let task_a = core
        .send_message(&a, "A private input", Arc::new(Gated(txa, gate.clone())))
        .unwrap();
    let task_b = core
        .send_message(&b.id, "B private input", Arc::new(Gated(txb, gate)))
        .unwrap();
    assert_eq!(
        rxa.recv_timeout(Duration::from_secs(2)).unwrap().kind,
        ChatEventKind::Started
    );
    assert_eq!(
        rxb.recv_timeout(Duration::from_secs(2)).unwrap().kind,
        ChatEventKind::Started
    );
    task_a.cancel();
    assert_eq!(
        terminal(&rxa).last().unwrap().kind,
        ChatEventKind::Cancelled
    );
    assert_eq!(
        terminal(&rxb).last().unwrap().kind,
        ChatEventKind::Completed
    );
    assert_eq!(
        core.messages(&b.id).unwrap()[1].status,
        MessageStatus::Complete
    );
    assert!(
        core.model_request(&task_b.id)
            .unwrap()
            .items
            .iter()
            .all(|i| !String::from_utf8_lossy(&i.payload).contains("A private input"))
    );
}

#[test]
fn creation_and_migration_preserve_files_ids_and_native_history() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai \"quoted\" 🪨").unwrap();
    assert_ne!(a, b.id);
    assert!(!b.conversation_id.is_empty());
    for file in ["config.toml", "SOUL.md", "MEMORY.md"] {
        assert!(std::path::Path::new(&b.directory).join(file).is_file());
    }
    assert!(core.create_persona("  ").is_err());
    let (task, rx) = start(&core, &a, "preserve input");
    terminal(&rx);
    let request = core.model_request(&task.id).unwrap();
    drop(task);
    drop(core);
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    raw_history(root.path());
    db.execute_batch("DROP TABLE persona_states; ALTER TABLE model_sessions DROP COLUMN response_schema; ALTER TABLE model_calls DROP COLUMN state_before; ALTER TABLE model_calls DROP COLUMN validation_errors; ALTER TABLE model_calls DROP COLUMN retry_of; ALTER TABLE model_items DROP COLUMN decoded_length; ALTER TABLE model_sessions DROP COLUMN context_reason; PRAGMA user_version=3;").unwrap();
    let core = reopen(root.path());
    let reopened = core
        .workspace()
        .unwrap()
        .personas
        .into_iter()
        .find(|p| p.id == b.id)
        .unwrap();
    assert_eq!(reopened.conversation_id, b.conversation_id);
    assert_eq!(reopened.display_name, b.display_name);
    let replay = core.model_request(&request.call_id).unwrap();
    for (a, b) in request.items.iter().zip(replay.items) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.payload, b.payload);
    }
    // FK enforcement on the reopened runtime connection is exercised by a normal valid turn.
    let (_task, rx) = start(&core, &reopened.id, "Kai only");
    terminal(&rx);
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}

#[test]
fn dropping_core_cancels_active_and_queued_work() {
    let (root, core, id) = isolated();
    let (active, rx) = start(&core, &id, "active");
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (queued, qrx) = start(&core, &id, "queued");
    drop(core);
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Cancelled);
    assert_eq!(
        terminal(&qrx).last().unwrap().kind,
        ChatEventKind::Cancelled
    );
    drop(active);
    drop(queued);
    let core = reopen(root.path());
    assert!(
        core.messages(&id)
            .unwrap()
            .iter()
            .filter(|m| m.sender_kind == "persona")
            .all(|m| m.status == MessageStatus::Cancelled)
    );
}

#[test]
fn restart_marks_queued_and_active_calls_interrupted_without_ghost_input() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "queued_crash_fixture"])
            .env("PEBBLE_QUEUE_CRASH_ROOT", root.path())
            .status()
            .unwrap()
            .success()
    );
    let core = reopen(root.path());
    let id = core.workspace().unwrap().personas[0].id.clone();
    let messages = core.messages(&id).unwrap();
    assert_eq!(messages[1].status, MessageStatus::Interrupted);
    assert_eq!(messages[3].status, MessageStatus::Interrupted);
    assert!(messages[3].content.is_empty());
    let (task, rx) = start(&core, &id, "after restart");
    terminal(&rx);
    assert!(
        core.model_request(&task.id)
            .unwrap()
            .items
            .iter()
            .all(|i| !String::from_utf8_lossy(&i.payload).contains("unconsumed queued prompt"))
    );
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM model_calls WHERE status='interrupted' AND finished_at IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}

#[test]
fn queued_crash_fixture() {
    let Some(root) = std::env::var_os("PEBBLE_QUEUE_CRASH_ROOT") else {
        return;
    };
    let core = pebble_core::Core::open(root).unwrap();
    let id = core.workspace().unwrap().personas[0].id.clone();
    let (_active, rx) = start(&core, &id, "before crash");
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let (_queued, _) = start(&core, &id, "unconsumed queued prompt");
    std::process::exit(0);
}

#[test]
fn queued_connection_change_never_imports_late_output_from_old_provider() {
    let old = Server::new(vec![Plan {
        status: 200,
        body: success("OLD_PROVIDER_OUTPUT"),
        delay_headers: false,
        delay_tail: true,
    }]);
    let new = Server::new(vec![
        stream(success("new first")),
        stream(success("new next")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &old.url, "old-model", "chat_completions", "");
    let (a, rx) = start(&core, &id, "OLD_PROVIDER_INPUT");
    old.requests.recv_timeout(Duration::from_secs(3)).unwrap();
    configure(root.path(), &new.url, "new-model", "chat_completions", "");
    let (b, brx) = start(&core, &id, "new input");
    terminal(&rx);
    terminal(&brx);
    let (c, crx) = start(&core, &id, "new continuation");
    terminal(&crx);
    let first = core.model_request(&a.id).unwrap();
    let next = core.model_request(&c.id).unwrap();
    assert_ne!(first.session_id, next.session_id);
    assert_eq!(
        next.session_id,
        core.model_request(&b.id).unwrap().session_id
    );
    assert_eq!(next.items.len(), 4);
    assert!(
        next.items
            .iter()
            .all(|item| !String::from_utf8_lossy(&item.payload).contains("OLD_PROVIDER"))
    );
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let covered: i64 = db
        .query_row(
            "SELECT covered_event_sequence FROM model_sessions WHERE id=?1",
            [&next.session_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        covered,
        db.query_row("SELECT MAX(sequence) FROM persona_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap()
    );
}
