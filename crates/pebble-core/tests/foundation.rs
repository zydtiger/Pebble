use pebble_core::{ChatEvent, ChatEventKind, ChatListener, Core, Error};
use std::fs;
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[test]
fn restart_keeps_identity_and_state_table() {
    let root = tempfile::tempdir().unwrap();
    let first = Core::open(root.path()).unwrap().workspace().unwrap();
    let second = Core::open(root.path()).unwrap().workspace().unwrap();
    assert_eq!(first.personas.len(), 1);
    assert_eq!(first.personas[0].id, second.personas[0].id);
    assert_eq!(
        first.personas[0].conversation_id,
        second.personas[0].conversation_id
    );
    assert!(first.personas[0].chat.is_none());
    assert!(first.personas[0].classifier.is_none());
    assert!(second.personas[0].chat.is_none());
    assert!(second.personas[0].classifier.is_none());
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        tables,
        [
            "context_maintenance",
            "context_marker_rooms",
            "context_markers",
            "conversation_memberships",
            "conversations",
            "delegated_tasks",
            "delegation_assignments",
            "delegation_runs",
            "delegation_submissions",
            "group_reply_slots",
            "group_rounds",
            "image_assets",
            "memory_operations",
            "message_images",
            "message_mentions",
            "messages",
            "model_calls",
            "model_item_events",
            "model_items",
            "model_sessions",
            "participation_decisions",
            "persona_events",
            "persona_states",
            "personas"
        ]
    );
}

#[test]
fn manual_edits_reload_without_reseeding_or_changing_room() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let before = core.workspace().unwrap().personas.remove(0);
    let directory = std::path::Path::new(&before.directory);
    fs::write(
        directory.join("MEMORY.md"),
        "# Memory\n\nLikes rainy walks.\n",
    )
    .unwrap();
    fs::write(
        directory.join("config.toml"),
        "[persona]\ndisplay_name = '米拉'\n[response.initial_state]\naffinity_user=20\nmood='calm'\n",
    )
    .unwrap();
    let after = core.workspace().unwrap().personas.remove(0);
    assert_eq!(after.display_name, "米拉");
    assert!(after.memory.contains("rainy walks"));
    assert_eq!(before.id, after.id);
    assert_eq!(before.conversation_id, after.conversation_id);
    drop(core);
    assert_eq!(
        Core::open(root.path())
            .unwrap()
            .workspace()
            .unwrap()
            .personas[0]
            .display_name,
        "米拉"
    );
}

#[test]
fn connections_inherit_as_a_whole_and_reject_partial_override() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    fs::write(root.path().join("config.toml"),
        "[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='sample'\nbase_url='http://localhost:1234/v1'\napi='responses'\napi_key_env='MODEL_KEY'\n").unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let chat = persona.chat.unwrap();
    assert_eq!(chat.api, "responses");
    assert_eq!(chat.api_key_env.as_deref(), Some("MODEL_KEY"));
    fs::write(
        std::path::Path::new(&persona.directory).join("config.toml"),
        "[persona]\ndisplay_name='Mira'\n[chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='other'\n",
    )
    .unwrap();
    assert!(matches!(core.workspace(), Err(Error::Configuration { .. })));
}

#[test]
fn inline_api_key_accepts_blank_and_redacts_nonempty_values() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let config = root.path().join("config.toml");
    let connection = "[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='sample'\nbase_url='http://localhost:1234/v1'\napi='chat_completions'\n";
    for key in ["", "test-only-placeholder"] {
        fs::write(&config, format!("{connection}api_key='{key}'\n")).unwrap();
        let chat = core.workspace().unwrap().personas.remove(0).chat.unwrap();
        assert_eq!(chat.api_key.as_deref(), Some(key));
        assert!(chat.api_key_env.is_none());
        assert!(!format!("{chat:?}").contains("test-only-placeholder"));
    }
    fs::write(
        &config,
        format!("{connection}api_key='test-only-placeholder'\napi_key_env='MODEL_KEY'\n"),
    )
    .unwrap();
    assert!(matches!(core.workspace(), Err(Error::Configuration { .. })));
    fs::write(
        &config,
        format!("{connection}api_key=''\napi_key_env='MODEL_KEY'\n"),
    )
    .unwrap();
    assert!(core.workspace().is_ok());
}

#[test]
fn invalid_and_missing_files_remain_visible_errors() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    fs::write(
        root.path().join("config.toml"),
        "[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='test'\nbase_url='file:///tmp'\napi='responses'\n",
    )
    .unwrap();
    assert!(matches!(core.workspace(), Err(Error::Configuration { .. })));
    fs::remove_file(root.path().join("config.toml")).unwrap();
    drop(core);
    assert!(matches!(Core::open(root.path()), Err(Error::File { .. })));
    assert!(!root.path().join("config.toml").exists());
}

struct Observer {
    events: mpsc::Sender<ChatEvent>,
    released: mpsc::Sender<()>,
}

impl ChatListener for Observer {
    fn on_event(&self, event: ChatEvent) -> Result<(), String> {
        self.events.send(event).map_err(|error| error.to_string())
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        let _ = self.released.send(());
    }
}

fn observer() -> (
    Arc<dyn ChatListener>,
    mpsc::Receiver<ChatEvent>,
    mpsc::Receiver<()>,
) {
    let (events, incoming) = mpsc::channel();
    let (released, dropped) = mpsc::channel();
    (Arc::new(Observer { events, released }), incoming, dropped)
}

fn receive_until_terminal(events: &mpsc::Receiver<ChatEvent>) -> Vec<ChatEvent> {
    let mut received = Vec::new();
    loop {
        let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
        let terminal = matches!(
            event.kind,
            ChatEventKind::Completed | ChatEventKind::Cancelled | ChatEventKind::Failed
        );
        received.push(event);
        if terminal {
            return received;
        }
    }
}

#[test]
fn stream_keeps_unicode_order_and_origin_and_releases_listener() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (observer, incoming, released) = observer();
    let task = core.send_message(&persona.id, "你好", observer).unwrap();
    let events = receive_until_terminal(&incoming);
    assert_eq!(events[0].kind, ChatEventKind::Started);
    assert_eq!(events.last().unwrap().kind, ChatEventKind::Completed);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.sequence, index as u64);
        assert_eq!(event.run_id, task.id);
        assert_eq!(event.conversation_id, persona.conversation_id);
        assert_eq!(event.persona_id, persona.id);
    }
    assert!(
        events
            .iter()
            .all(|event| event.kind != ChatEventKind::TextDelta)
    );
    let text = &events.last().unwrap().messages[0].content;
    assert!(text.starts_with("本地演示 · 第 1 次对话"));
    assert!(text.ends_with("没有调用模型。"));
    released.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(incoming.try_recv().is_err());
}

#[test]
fn cancellation_stops_stream_and_allows_another_preview() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (first_observer, incoming, released) = observer();
    let task = core
        .send_message(&persona.id, "hello", first_observer)
        .unwrap();
    assert_eq!(
        incoming.recv_timeout(Duration::from_secs(1)).unwrap().kind,
        ChatEventKind::Started
    );
    let (other, queued_events, _) = observer();
    let queued = core.send_message(&persona.id, "again", other).unwrap();
    assert_eq!(
        queued.messages[1].status,
        pebble_core::MessageStatus::Queued
    );
    queued.cancel();
    assert_eq!(
        receive_until_terminal(&queued_events).last().unwrap().kind,
        ChatEventKind::Cancelled
    );
    task.cancel();
    task.cancel();
    assert_eq!(
        receive_until_terminal(&incoming).last().unwrap().kind,
        ChatEventKind::Cancelled
    );
    released.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(incoming.try_recv().is_err());
    let (next, _, released) = observer();
    let next_task = core.send_message(&persona.id, "again", next).unwrap();
    assert_ne!(task.id, next_task.id);
    drop(next_task);
    released.recv_timeout(Duration::from_secs(1)).unwrap();
}

#[test]
fn dropping_handle_cancels_and_releases_callback() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (observer, incoming, released) = observer();
    let task = core.send_message(&persona.id, "hello", observer).unwrap();
    incoming.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(task);
    assert_eq!(
        receive_until_terminal(&incoming).last().unwrap().kind,
        ChatEventKind::Cancelled
    );
    released.recv_timeout(Duration::from_secs(1)).unwrap();
}

#[test]
fn preview_validates_input_and_persona() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let (listener, _, _) = observer();
    assert!(matches!(
        core.send_message("missing", "hello", listener.clone()),
        Err(Error::UnknownPersona)
    ));
    assert!(matches!(
        core.send_message("missing", "  ", listener),
        Err(Error::EmptyMessage)
    ));
}

#[test]
fn malformed_config_and_url_credentials_never_echo_secrets() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    for config in [
        "[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='mock'\nbase_url='http://127.0.0.1/v1'\napi='chat_completions'\napi_key='do-not-echo' broken",
        "[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='mock'\nbase_url='http://do-not-echo@127.0.0.1/v1'\napi='chat_completions'",
    ] {
        fs::write(root.path().join("config.toml"), config).unwrap();
        let error = core.workspace().unwrap_err().to_string();
        assert!(!error.contains("do-not-echo"));
    }
}
