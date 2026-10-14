mod migration_support;
use pebble_core::{ChatEvent, ChatEventKind, ChatListener, ChatTask, Core, Error, MessageStatus};
use rusqlite::Connection;
use serde_json::Value;
use std::sync::{Arc, mpsc};
use std::time::Duration;

struct Listener(mpsc::Sender<ChatEvent>);
impl ChatListener for Listener {
    fn on_event(&self, event: ChatEvent) -> Result<(), String> {
        self.0.send(event).map_err(|error| error.to_string())
    }
}
fn start(core: &Core, persona: &str, text: &str) -> (Arc<ChatTask>, mpsc::Receiver<ChatEvent>) {
    let (tx, rx) = mpsc::channel();
    (
        core.send_message(persona, text, Arc::new(Listener(tx)))
            .unwrap(),
        rx,
    )
}
fn finish(rx: &mpsc::Receiver<ChatEvent>) -> Vec<ChatEvent> {
    let mut result = Vec::new();
    while let Ok(event) = rx.recv_timeout(Duration::from_secs(5)) {
        result.push(event);
    }
    assert!(matches!(
        result.last().unwrap().kind,
        ChatEventKind::Completed | ChatEventKind::Cancelled
    ));
    result
}
fn db(root: &std::path::Path) -> Connection {
    Connection::open(root.join("data/app.sqlite")).unwrap()
}
fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

#[test]
fn restart_replays_frozen_prefix_and_links_own_output_once() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (first, events) = start(&core, &persona.id, "I like rainy walks.");
    finish(&events);
    let first_request = core.model_request(&first.id).unwrap();
    assert_eq!(first_request.items.len(), 2);
    let saved = core.messages(&persona.id).unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[1].status, MessageStatus::Complete);
    let reply_id = saved[1].id.clone();
    drop(first);
    drop(core);
    // The file lock is released with the database after the completed worker exits.
    let core = reopen(root.path());
    assert_eq!(saved, core.messages(&persona.id).unwrap());
    std::fs::write(
        std::path::Path::new(&persona.directory).join("SOUL.md"),
        "Changed after the first turn",
    )
    .unwrap();
    let (second, events) = start(&core, &persona.id, "What did I say?");
    finish(&events);
    let request = core.model_request(&second.id).unwrap();
    assert_eq!(request.session_id, first_request.session_id);
    assert_eq!(request.protocol, "pebble_demo_v1");
    assert_eq!(request.items.len(), 4);
    for (a, b) in request.items.iter().zip(&first_request.items) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.payload, b.payload);
    }
    let output: Value = serde_json::from_slice(&request.items[2].payload).unwrap();
    assert_eq!(output["role"], "assistant");
    assert_eq!(
        serde_json::from_str::<Value>(output["content"].as_str().unwrap()).unwrap()["reply"],
        saved[1].content
    );
    let last: Value = serde_json::from_slice(&request.items[3].payload).unwrap();
    let envelope: Value = serde_json::from_str(last["content"].as_str().unwrap()).unwrap();
    assert_eq!(envelope["events"].as_array().unwrap().len(), 1);
    assert_eq!(envelope["events"][0]["text"], "What did I say?");
    assert_eq!(
        envelope["reply_request"]["conversation_id"],
        persona.conversation_id
    );
    let messages = core.messages(&persona.id).unwrap();
    assert_eq!(
        messages.iter().map(|m| m.sequence).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert!(
        messages[3]
            .content
            .contains("Your previous message was: “I like rainy walks.”")
    );
    assert_eq!(
        core.model_request(&first_request.call_id)
            .unwrap()
            .items
            .len(),
        2
    );
    let db = db(root.path());
    assert_eq!(count(&db, "messages"), 4);
    assert_eq!(count(&db, "persona_events"), 4);
    assert_eq!(count(&db, "model_items"), 5);
    assert_eq!(count(&db, "model_item_events"), 4);
    assert_eq!(count(&db, "model_calls"), 2);
    let own_links: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM model_item_events x JOIN persona_events e ON e.id = x.event_id
         JOIN model_items i ON i.id = x.item_id WHERE e.message_id = ?1 AND i.kind = 'output'",
            [reply_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(own_links, 1);
    let frontier: i64 = db
        .query_row(
            "SELECT covered_event_sequence FROM model_sessions",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(frontier, 4);
    assert_eq!(
        db.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}

fn reopen(root: &std::path::Path) -> Core {
    for _ in 0..100 {
        match Core::open(root) {
            Ok(core) => return core,
            Err(Error::AlreadyOpen) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => panic!("{error}"),
        }
    }
    panic!("Database ownership was not released")
}

#[test]
fn cancellation_preserves_empty_pending_bubble_without_accepted_output() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (task, events) = start(&core, &persona.id, "A partial thought");
    let started = events.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(started.messages.len(), 2);
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        events
            .try_iter()
            .all(|e| e.kind != ChatEventKind::TextDelta)
    );
    assert!(core.messages(&persona.id).unwrap()[1].content.is_empty());
    task.cancel();
    let terminal = finish(&events);
    assert_eq!(terminal.last().unwrap().kind, ChatEventKind::Cancelled);
    let saved = core.messages(&persona.id).unwrap();
    assert_eq!(saved[1].status, MessageStatus::Cancelled);
    assert!(saved[1].content.is_empty());
    let (next, events) = start(&core, &persona.id, "Next thought");
    finish(&events);
    let request = core.model_request(&next.id).unwrap();
    assert_eq!(request.items.len(), 3);
    assert!(request.items.iter().all(
        |item| serde_json::from_slice::<Value>(&item.payload).unwrap()["role"] != "assistant"
    ));
    drop(task);
    drop(next);
    drop(core);
    let mut expected = saved[1].clone();
    // Retry eligibility changes when a later call exists; the saved attempt stays intact.
    assert!(expected.can_retry);
    expected.can_retry = false;
    assert_eq!(
        reopen(root.path()).messages(&persona.id).unwrap()[1],
        expected
    );
}

#[test]
fn acceptance_rolls_back_as_one_transaction() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let db = db(root.path());
    db.execute_batch("CREATE TRIGGER reject_item BEFORE INSERT ON model_calls BEGIN SELECT RAISE(ABORT, 'test rollback'); END;").unwrap();
    let (tx, _) = mpsc::channel();
    assert!(
        core.send_message(&persona.id, "hello", Arc::new(Listener(tx)))
            .is_err()
    );
    for table in [
        "messages",
        "persona_events",
        "model_sessions",
        "model_calls",
        "model_items",
        "model_item_events",
    ] {
        assert_eq!(count(&db, table), 0, "{table} was partially committed");
    }
    db.execute_batch("DROP TRIGGER reject_item;").unwrap();
    let (_task, events) = start(&core, &persona.id, "hello");
    finish(&events);
    assert_eq!(core.messages(&persona.id).unwrap()[0].sequence, 1);
}

#[test]
fn persona_history_is_isolated() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let mira = core.workspace().unwrap().personas.remove(0);
    let kai = core.create_persona("Kai").unwrap();
    let (_mira_task, events) = start(&core, &mira.id, "Mira's private fact");
    finish(&events);
    let (kai_task, events) = start(&core, &kai.id, "Hello Kai");
    finish(&events);
    let request = core.model_request(&kai_task.id).unwrap();
    assert!(
        !request
            .items
            .iter()
            .any(|item| String::from_utf8_lossy(&item.payload).contains("private fact"))
    );
    assert_eq!(core.messages(&kai.id).unwrap().len(), 2);
    assert_ne!(kai.conversation_id, mira.conversation_id);
}

#[test]
fn exclusive_owner_prevents_false_interruption_and_rejects_future_schema() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    assert!(matches!(Core::open(root.path()), Err(Error::AlreadyOpen)));
    drop(core);
    db(root.path())
        .execute_batch("PRAGMA user_version = 99")
        .unwrap();
    assert!(matches!(Core::open(root.path()), Err(Error::Schema(99))));
    assert_eq!(
        db(root.path())
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        99
    );
}

#[test]
fn v1_migration_keeps_persona_and_room_ids() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    drop(core);
    let db = db(root.path());
    migration_support::v7(root.path());
    db.execute_batch("DROP TABLE persona_states; ALTER TABLE model_sessions DROP COLUMN response_schema; ALTER TABLE model_calls DROP COLUMN state_before; ALTER TABLE model_calls DROP COLUMN validation_errors; DROP TABLE model_item_events; DROP TABLE model_items; DROP TABLE model_calls;
        DROP TABLE model_sessions; DROP TABLE persona_events; DROP TABLE messages; PRAGMA user_version = 1;").unwrap();
    let core = Core::open(root.path()).unwrap();
    let migrated = core.workspace().unwrap().personas.remove(0);
    assert_eq!(migrated.id, persona.id);
    assert_eq!(migrated.conversation_id, persona.conversation_id);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        16
    );
    assert!(core.messages(&persona.id).unwrap().is_empty());
}

#[test]
fn abrupt_exit_recovers_durable_delta_without_accepting_output() {
    let root = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_fixture", "--nocapture"])
        .env("PEBBLE_CRASH_FIXTURE", root.path())
        .status()
        .unwrap();
    assert!(status.success());
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let messages = core.messages(&persona.id).unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].status, MessageStatus::Interrupted);
    assert!(messages[1].content.is_empty());
    let db = db(root.path());
    assert_eq!(count(&db, "model_items"), 2);
    assert_eq!(
        db.query_row("SELECT status FROM model_calls", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "interrupted"
    );
    let (_task, events) = start(&core, &persona.id, "After restart");
    finish(&events);
    assert_eq!(core.messages(&persona.id).unwrap().len(), 4);
}

#[test]
fn crash_fixture() {
    let Some(path) = std::env::var_os("PEBBLE_CRASH_FIXTURE") else {
        return;
    };
    let core = Core::open(path).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (_task, events) = start(&core, &persona.id, "Before abrupt exit");
    events.recv_timeout(Duration::from_secs(1)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(core.messages(&persona.id).unwrap()[1].content.is_empty());
    std::process::exit(0);
}

#[test]
fn failed_publication_never_leaves_half_an_accepted_output() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let db = db(root.path());
    db.execute_batch(
        "CREATE TRIGGER reject_output BEFORE INSERT ON persona_events WHEN NEW.kind = 'sent'
        BEGIN SELECT RAISE(ABORT, 'test publication failure'); END;",
    )
    .unwrap();
    let (_task, events) = start(&core, &persona.id, "hello");
    let mut terminal = None;
    while let Ok(event) = events.recv_timeout(Duration::from_secs(5)) {
        terminal = Some(event);
    }
    assert_eq!(terminal.unwrap().kind, ChatEventKind::Failed);
    assert_eq!(
        core.messages(&persona.id).unwrap()[1].status,
        MessageStatus::Failed
    );
    assert_eq!(count(&db, "persona_events"), 1);
    assert_eq!(count(&db, "model_items"), 2);
    assert_eq!(count(&db, "model_item_events"), 1);
    assert_eq!(
        db.query_row("SELECT status FROM model_calls", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "failed"
    );
    db.execute_batch("DROP TRIGGER reject_output;").unwrap();
    let (_next, events) = start(&core, &persona.id, "try a new turn");
    finish(&events);
}
