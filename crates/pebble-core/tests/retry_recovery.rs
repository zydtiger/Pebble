mod live_support;
use live_support::{isolated, reopen, retry, start, terminal};
use pebble_core::{ChatEventKind, MessageStatus};
use std::time::Duration;
#[test]
fn interrupted_queued_fixture() {
    let Ok(root) = std::env::var("PEBBLE_RETRY_CRASH_ROOT") else {
        return;
    };
    let core = pebble_core::Core::open(root).unwrap();
    let id = core.workspace().unwrap().personas[0].id.clone();
    let (_first, rx) = start(&core, &id, "activated before crash");
    let (_queued, _qrx) = start(&core, &id, "queued before crash");
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap().kind,
        ChatEventKind::Started
    );
    std::thread::sleep(Duration::from_millis(100));
    std::process::exit(0);
}
#[test]
fn restart_exposes_interrupted_queued_retry_without_duplicating_received_events() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "interrupted_queued_fixture", "--nocapture"])
            .env("PEBBLE_RETRY_CRASH_ROOT", root.path())
            .status()
            .unwrap()
            .success()
    );
    let core = reopen(root.path());
    let id = core.workspace().unwrap().personas[0].id.clone();
    let saved = core.messages(&id).unwrap();
    assert_eq!(saved.len(), 4);
    assert_eq!(saved[1].status, MessageStatus::Interrupted);
    assert!(saved[1].content.is_empty());
    assert_eq!(saved[3].status, MessageStatus::Interrupted);
    assert!(saved[3].can_retry);
    assert!(saved[3].failure_detail.contains("Application stopped"));
    let (attempt, rx) = retry(&core, &id, &saved[3].id);
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    let request = core.model_request(&attempt.id).unwrap();
    assert_eq!(request.items.len(), 3);
    assert!(String::from_utf8_lossy(&request.items[2].payload).contains("queued before crash"));
    assert_eq!(core.messages(&id).unwrap().len(), 5);
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM persona_events WHERE kind='received'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}
#[test]
fn edited_persona_files_only_change_native_prefix_after_explicit_fresh_context() {
    let (_root, core, id) = isolated();
    let (first, rx) = start(&core, &id, "first");
    terminal(&rx);
    let before = core.model_request(&first.id).unwrap();
    let settings = core.settings(Some(&id)).unwrap();
    let mut persona = settings.persona.unwrap();
    persona.soul = "new edited SOUL".into();
    persona.memory = "new edited MEMORY".into();
    persona.display_name = "Renamed".into();
    core.save_settings(
        Some(&id),
        &pebble_core::SettingsUpdate {
            chat: settings.chat,
            credential: pebble_core::CredentialChange::Keep,
            persona: Some(persona),
        },
    )
    .unwrap();
    let (next, rx) = start(&core, &id, "same context");
    terminal(&rx);
    assert_eq!(
        core.model_request(&next.id).unwrap().items[0].payload,
        before.items[0].payload
    );
    core.fresh_context(&id).unwrap();
    let (fresh, rx) = start(&core, &id, "fresh");
    terminal(&rx);
    let prefix = core.model_request(&fresh.id).unwrap().items[0]
        .payload
        .clone();
    assert!(String::from_utf8_lossy(&prefix).contains("new edited SOUL"));
    assert!(String::from_utf8_lossy(&prefix).contains("new edited MEMORY"));
}
