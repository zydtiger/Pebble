mod live_support;
use live_support::{Server, configure, isolated, start, stream, success, terminal};
use pebble_core::{Core, CredentialChange, Error, SettingsUpdate};
use std::fs;
fn update(core: &Core, persona: Option<&str>) -> SettingsUpdate {
    let s = core.settings(persona).unwrap();
    SettingsUpdate {
        chat: s.chat,
        credential: CredentialChange::Keep,
        context_window_tokens: s.context_window_tokens,
        max_output_tokens: s.max_output_tokens,
        persona: s.persona,
    }
}
#[test]
fn minimal_inherited_settings_read_safely_and_keep_clear_credentials_without_readback() {
    let (root, core, id) = isolated();
    assert!(!core.settings(Some(&id)).unwrap().chat.enabled);
    fs::write(root.path().join("config.toml"), "# minimal app\n").unwrap();
    assert!(!core.settings(None).unwrap().chat.enabled);
    let server = Server::new(vec![stream(success("first")), stream(success("second"))]);
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let mut settings = update(&core, None);
    settings.credential = CredentialChange::Replace("synthetic-secret".into());
    core.save_settings(None, &settings).unwrap();
    assert!(core.settings(None).unwrap().chat.has_api_key);
    assert!(!format!("{:?}", core.settings(None).unwrap()).contains("synthetic-secret"));
    assert!(!format!("{settings:?}").contains("synthetic-secret"));
    let mut unrelated = update(&core, None);
    unrelated.max_output_tokens = Some(128);
    core.save_settings(None, &unrelated).unwrap();
    let (_task, rx) = start(&core, &id, "first");
    terminal(&rx);
    let (headers, _) = server.requests.recv().unwrap();
    assert!(
        headers
            .to_lowercase()
            .contains("authorization: bearer synthetic-secret")
    );
    unrelated.credential = CredentialChange::Clear;
    core.save_settings(None, &unrelated).unwrap();
    let (_task, rx) = start(&core, &id, "second");
    terminal(&rx);
    assert!(
        !server
            .requests
            .recv()
            .unwrap()
            .0
            .to_lowercase()
            .contains("authorization:")
    );
    assert!(!core.settings(None).unwrap().chat.has_api_key);
    unrelated.credential = CredentialChange::Environment("SYNTHETIC_PEBBLE_KEY".into());
    core.save_settings(None, &unrelated).unwrap();
    assert_eq!(
        core.settings(None).unwrap().chat.api_key_env,
        "SYNTHETIC_PEBBLE_KEY"
    );
    unrelated.credential = CredentialChange::Replace(String::new());
    assert!(core.save_settings(None, &unrelated).is_err());
}
#[test]
fn candidate_validation_preserves_unknown_fields_and_rejects_dependent_invalid_budgets_before_write()
 {
    let (root, core, id) = isolated();
    let persona = core.workspace().unwrap().personas.remove(0);
    let path = std::path::Path::new(&persona.directory).join("config.toml");
    fs::write(&path,"# character comment\n[persona]\ndisplay_name='Mira' # name comment\nsubtitle='Here'\nunknown='retained'\n[context]\nmax_output_tokens=5000\n[custom]\nopaque='keep me'\n").unwrap();
    let before = fs::read(root.path().join("config.toml")).unwrap();
    let mut app = update(&core, None);
    app.context_window_tokens = Some(4500);
    app.max_output_tokens = Some(100);
    assert!(matches!(
        core.save_settings(None, &app),
        Err(Error::Configuration { .. })
    ));
    assert_eq!(fs::read(root.path().join("config.toml")).unwrap(), before);
    let mut edited = update(&core, Some(&id));
    edited.persona.as_mut().unwrap().display_name = "Renamed".into();
    core.save_settings(Some(&id), &edited).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    for text in [
        "# character comment",
        "# name comment",
        "unknown='retained'",
        "opaque='keep me'",
    ] {
        assert!(saved.contains(text), "{saved}");
    }
    assert_eq!(
        core.workspace().unwrap().personas[0].display_name,
        "Renamed"
    );
    edited.chat.enabled = true;
    edited.chat.model = "mock".into();
    edited.chat.base_url = "http://localhost:9/v1".into();
    edited.chat.api = "bad".into();
    edited.credential = CredentialChange::Replace("hidden-invalid-secret".into());
    let failed = core
        .save_settings(Some(&id), &edited)
        .unwrap_err()
        .to_string();
    assert!(!failed.contains("hidden-invalid-secret"));
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
}
#[test]
fn failed_staging_changes_no_files_and_busy_rules_only_block_affected_scope() {
    let (root, core, id) = isolated();
    let b = core.create_persona("Kai").unwrap();
    let (task, rx) = start(&core, &id, "keep A active");
    assert!(matches!(
        core.save_settings(None, &update(&core, None)),
        Err(Error::Busy)
    ));
    assert!(matches!(
        core.save_settings(Some(&id), &update(&core, Some(&id))),
        Err(Error::Busy)
    ));
    core.save_settings(Some(&b.id), &update(&core, Some(&b.id)))
        .unwrap();
    task.cancel();
    terminal(&rx);
    let mut edited = update(&core, Some(&id));
    edited.persona.as_mut().unwrap().soul = "must not publish".into();
    let directory = root.path().join("personas").join(&id);
    let before = fs::read(directory.join("SOUL.md")).unwrap();
    fs::remove_file(directory.join("MEMORY.md")).unwrap();
    fs::create_dir(directory.join("MEMORY.md")).unwrap();
    let error = core
        .save_settings(Some(&id), &edited)
        .unwrap_err()
        .to_string();
    // Current files must be readable before editing; either read/validation or staging fails safely.
    assert!(!error.is_empty());
    assert_eq!(fs::read(directory.join("SOUL.md")).unwrap(), before);
    assert!(!fs::read_dir(directory).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pebble-save-")
    }));
}
