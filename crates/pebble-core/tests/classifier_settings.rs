mod live_support;
use live_support::isolated;
use pebble_core::{CredentialChange, Error};

#[test]
fn independent_inheritance_credentials_comments_and_protocol_switches_round_trip() {
    let (root, core, id) = isolated();
    std::fs::write(root.path().join("config.toml"),"# authored app comment\n[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='chat-unchanged'\nbase_url='http://localhost:8080/v1'\napi='chat_completions'\napi_key='chat-secret'\n[custom]\nopaque='keep'\n").unwrap();
    let chat = core.settings(None).unwrap();
    for persona in [None, Some(id.as_str())] {
        let mut s = core.classifier_settings(persona).unwrap();
        assert!(!s.connection.enabled);
        s.connection.enabled = true;
        s.connection.context_len = Some(16384);
        s.connection.max_output_tokens = Some(64);
        s.connection.model = "classifier".into();
        s.connection.base_url = "http://localhost:9090/v1".into();
        s.connection.request_json =
            r#"{"temperature":0,"chat_template_kwargs":{"enable_thinking":false}}"#.into();
        core.save_classifier_settings(
            persona,
            &s,
            &CredentialChange::Replace("synthetic-classifier-key".into()),
        )
        .unwrap();
        let read = core.classifier_settings(persona).unwrap();
        assert!(read.connection.has_api_key);
        assert_eq!(read.connection.context_len, Some(16384));
        assert_eq!(read.connection.max_output_tokens, Some(64));
        assert!(!format!("{read:?}").contains("synthetic-classifier-key"));
        assert_eq!(
            core.workspace().unwrap().personas[0]
                .classifier
                .as_ref()
                .unwrap()
                .connection
                .model,
            "classifier"
        );
        for dialect in ["text_format", "response_format"] {
            // Native forms retain hidden controls in memory; saving must clear inapplicable fields.
            s.connection.api = "responses".into();
            s.connection.responses_schema_format = dialect.into();
            core.save_classifier_settings(persona, &s, &CredentialChange::Keep)
                .unwrap();
            let read = core.classifier_settings(persona).unwrap();
            assert!(read.connection.request_json.contains("enable_thinking"));
            assert_eq!(read.connection.responses_schema_format, dialect);
            s.connection.api = "chat_completions".into();
            core.save_classifier_settings(persona, &s, &CredentialChange::Keep)
                .unwrap();
            let read = core.classifier_settings(persona).unwrap();
            assert!(read.connection.request_json.contains("enable_thinking"));
            assert!(read.connection.has_api_key);
            let path = if let Some(id) = persona {
                root.path().join("personas").join(id).join("config.toml")
            } else {
                root.path().join("config.toml")
            };
            let doc: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let table = if persona.is_some() {
                &doc["classifier"]
            } else {
                &doc["defaults"]["classifier"]
            };
            assert!(table.get("responses_schema_format").is_none());
        }
        s.connection.request_json = r#"{"stream":false}"#.into();
        assert!(matches!(
            core.save_classifier_settings(persona, &s, &CredentialChange::Keep),
            Err(Error::SettingsSave(_))
        ));
        s.connection.request_json = "{}".into();
        core.save_classifier_settings(
            persona,
            &s,
            &CredentialChange::Environment("CLASSIFIER_TEST_KEY".into()),
        )
        .unwrap();
        assert_eq!(
            core.classifier_settings(persona)
                .unwrap()
                .connection
                .api_key_env,
            "CLASSIFIER_TEST_KEY"
        );
        core.save_classifier_settings(persona, &s, &CredentialChange::Clear)
            .unwrap();
        assert!(
            !core
                .classifier_settings(persona)
                .unwrap()
                .connection
                .has_api_key
        );
    }
    let after = core.settings(None).unwrap();
    assert_eq!(after.chat.model, chat.chat.model);
    assert!(after.chat.has_api_key);
    let authored = std::fs::read_to_string(root.path().join("config.toml")).unwrap();
    assert!(
        authored.contains("# authored app comment")
            && authored.contains("opaque='keep'")
            && authored.contains("chat-secret")
    );
    let mut persona = core.classifier_settings(Some(&id)).unwrap();
    persona.connection.enabled = false;
    core.save_classifier_settings(Some(&id), &persona, &CredentialChange::Keep)
        .unwrap();
    assert!(
        !core
            .classifier_settings(Some(&id))
            .unwrap()
            .connection
            .enabled
    );
    assert!(core.workspace().unwrap().personas[0].classifier.is_some());
    let mut app = core.classifier_settings(None).unwrap();
    app.connection.enabled = false;
    core.save_classifier_settings(None, &app, &CredentialChange::Keep)
        .unwrap();
    assert!(core.workspace().unwrap().personas[0].classifier.is_none());
}

#[test]
fn obsolete_request_mode_is_a_recoverable_config_error() {
    let (root, core, _) = isolated();
    std::fs::write(root.path().join("config.toml"),"[defaults.classifier]\ncontext_len=16384\nmax_output_tokens=64\nmodel='synthetic'\nbase_url='http://localhost/v1'\napi='responses'\nrequest_profile='obsolete'\n").unwrap();
    assert!(matches!(core.workspace(), Err(Error::Configuration { .. })));
}
