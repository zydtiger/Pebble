mod classifier_support;
mod live_support;
mod request_parameter_toml;
use live_support::{Server, configure, isolated, start, stream, success, terminal};
use pebble_core::{CredentialChange, SettingsUpdate};
use serde_json::{Value, json};

fn save(
    core: &pebble_core::Core,
    persona: Option<&str>,
    request: Value,
) -> Result<(), pebble_core::Error> {
    let settings = core.settings(persona)?;
    core.save_settings(
        persona,
        &SettingsUpdate {
            chat: pebble_core::ChatSettings {
                request_json: request.to_string(),
                ..settings.chat
            },
            credential: CredentialChange::Keep,
            persona: settings.persona,
        },
    )?;
    Ok(())
}

#[test]
fn typed_nested_settings_roundtrip_inherit_as_whole_and_reject_before_publication() {
    let (root, core, id) = isolated();
    configure(
        root.path(),
        "http://localhost:9/v1",
        "synthetic",
        "chat_completions",
        "",
    );
    let params = json!({"temperature":0,"top_p":0.75,"seed":0,"chat_template_kwargs":{"enable_thinking":false,"vendor_template":{"literal":[0,false,"你好"]}},"vendor":{"arrays":[[1,2],[3]],"empty":{}}});
    save(&core, None, params.clone()).unwrap();
    let read: Value =
        serde_json::from_str(&core.settings(None).unwrap().chat.request_json).unwrap();
    assert_eq!(read, params);
    assert_eq!(
        Value::Object(
            core.workspace().unwrap().personas[0]
                .chat
                .as_ref()
                .unwrap()
                .request
                .clone()
        ),
        params
    );
    let mut override_settings = core.settings(Some(&id)).unwrap();
    override_settings.chat = core.settings(None).unwrap().chat;
    override_settings.chat.request_json = "{}".into();
    core.save_settings(
        Some(&id),
        &SettingsUpdate {
            chat: override_settings.chat,
            credential: CredentialChange::Clear,
            persona: override_settings.persona,
        },
    )
    .unwrap();
    assert!(
        core.workspace().unwrap().personas[0]
            .chat
            .as_ref()
            .unwrap()
            .request
            .is_empty()
    );
    let path = root.path().join("config.toml");
    let before = std::fs::read(&path).unwrap();
    for request in [
        json!({"stream":false}),
        json!({"temperature":3}),
        json!({"temperature":"0"}),
        json!({"chat_template_kwargs":{"enable_thinking":"false"}}),
        json!({"chat_template_kwargs":{"tools":[]}}),
        json!({"reasoning_effort":"low","chat_template_kwargs":{"enable_thinking":false}}),
        json!({"seed":null}),
        json!({"seed":18446744073709551615u64}),
        json!([]),
    ] {
        assert!(save(&core, None, request.clone()).is_err(), "{request}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    save(&core, None, json!({})).unwrap();
    assert!(
        !std::fs::read_to_string(path)
            .unwrap()
            .contains("[defaults.chat.request]")
    );
}

#[test]
fn sampling_and_thinking_switch_reuse_context_but_template_semantics_retire_it() {
    let server = Server::new((0..4).map(|_| stream(success("accepted"))).collect());
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let mut sessions = Vec::new();
    for request in [
        json!({}),
        json!({"temperature":0,"chat_template_kwargs":{"enable_thinking":false}}),
        json!({"temperature":0.5,"chat_template_kwargs":{"enable_thinking":true}}),
        json!({"chat_template_kwargs":{"enable_thinking":true,"preserve_thinking":true}}),
    ] {
        save(&core, None, request.clone()).unwrap();
        let (task, rx) = start(&core, &id, "synthetic input");
        assert_eq!(
            terminal(&rx).last().unwrap().kind,
            pebble_core::ChatEventKind::Completed
        );
        sessions.push(core.model_request(&task.id).unwrap().session_id);
        let body = server.requests.recv().unwrap().1;
        assert_eq!(body.get("temperature"), request.get("temperature"));
        assert_eq!(
            body.get("chat_template_kwargs"),
            request.get("chat_template_kwargs")
        );
        assert!(body.get("request").is_none());
    }
    assert_eq!(sessions[0], sessions[1]);
    assert_eq!(sessions[1], sessions[2]);
    assert_ne!(sessions[2], sessions[3]);
}

#[test]
fn accepted_work_and_format_repair_freeze_parameters_even_after_manual_file_edit() {
    let server = Server::new(vec![
        live_support::Plan {
            status: 200,
            body: live_support::candidate("not-json"),
            delay_headers: true,
            delay_tail: false,
        },
        stream(success("repaired")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    save(
        &core,
        None,
        json!({"temperature":0,"chat_template_kwargs":{"enable_thinking":false}}),
    )
    .unwrap();
    let (_task, rx) = start(&core, &id, "repair");
    let first = server.requests.recv().unwrap().1;
    assert!(matches!(
        save(&core, None, json!({"temperature":1})),
        Err(pebble_core::Error::Busy)
    ));
    let path = root.path().join("config.toml");
    let mut doc: toml_edit::DocumentMut = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    doc["defaults"]["chat"]["request"]["temperature"] = toml_edit::value(1);
    std::fs::write(path, doc.to_string()).unwrap();
    assert_eq!(
        terminal(&rx).last().unwrap().kind,
        pebble_core::ChatEventKind::Completed
    );
    let repaired = server.requests.recv().unwrap().1;
    assert_eq!(first["temperature"], 0);
    assert_eq!(repaired["temperature"], 0);
    assert_eq!(repaired["chat_template_kwargs"]["enable_thinking"], false);
    assert!(repaired["response_format"].is_object());
}

#[test]
#[ignore = "requires an explicitly selected live classifier; synthetic isolated root only"]
fn opt_in_live_classifier_concrete_parameters() {
    let (_root, core, id) = isolated();
    let mut settings = core.classifier_settings(None).unwrap();
    settings.connection.enabled = true;
    settings.connection.base_url =
        std::env::var("PEBBLE_CLASSIFIER_API_ROOT").expect("set classifier API root");
    settings.connection.model =
        std::env::var("PEBBLE_CLASSIFIER_MODEL").expect("set classifier model");
    settings.connection.api = "chat_completions".into();
    settings.connection.context_len = Some(16384);
    settings.connection.max_output_tokens = Some(64);
    settings.connection.request_json =
        r#"{"temperature":0,"chat_template_kwargs":{"enable_thinking":false}}"#.into();
    core.save_classifier_settings(None, &settings, &CredentialChange::Clear)
        .unwrap();
    let group = core
        .create_group("Synthetic request parameters", std::slice::from_ref(&id))
        .unwrap();
    let (submission, rx) = classifier_support::submit(
        &core,
        &group.id,
        "Synthetic math question: does everyone agree that 2 + 2 equals 4?",
        &[],
    );
    classifier_support::finish(&submission, &rx);
    let decisions = core.participation(&group.id).unwrap();
    assert!(!decisions.is_empty());
    for decision in decisions {
        assert_ne!(decision.status, "failed", "{}", decision.error);
        assert!(decision.error.is_empty());
        println!(
            "Synthetic live classifier: phase={} status={} respond={:?}",
            decision.phase, decision.status, decision.respond
        );
    }
    drop(submission);
}
