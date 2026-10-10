mod live_support;
mod responses_support;
use live_support::{Server, configure, isolated, reopen, start, stream, success, terminal};
use pebble_core::{ChatEventKind, ContextBudget, Error, MessageStatus};
use std::fs;

fn connection(table: &str, model: &str, limits: &str) -> String {
    format!(
        "[{table}]\nmodel='{model}'\nbase_url='http://localhost:9/v1'\napi='chat_completions'\n{limits}\n"
    )
}

#[test]
fn whole_connections_inherit_their_own_limits_and_invalid_or_obsolete_limits_fail() {
    let (root, core, id) = isolated();
    assert_eq!(
        core.workspace().unwrap().personas[0].context,
        ContextBudget::default()
    );
    let app = root.path().join("config.toml");
    let persona = root.path().join("personas").join(&id).join("config.toml");
    let original = fs::read_to_string(&persona).unwrap();
    let configured = format!(
        "{}{}",
        connection(
            "defaults.chat",
            "chat",
            "context_len=10000\nmax_output_tokens=500"
        ),
        connection(
            "defaults.classifier",
            "classifier",
            "context_len=6000\nmax_output_tokens=77"
        )
    );
    fs::write(&app, &configured).unwrap();
    let p = core.workspace().unwrap().personas.remove(0);
    assert_eq!(
        p.context,
        ContextBudget {
            context_len: 10000,
            max_output_tokens: 500
        }
    );
    assert_eq!(p.classifier.unwrap().connection.max_output_tokens, 77);
    assert_eq!(
        core.settings(Some(&id)).unwrap().chat.context_len,
        Some(10000)
    );
    assert_eq!(
        core.classifier_settings(Some(&id))
            .unwrap()
            .connection
            .max_output_tokens,
        Some(77)
    );
    for name in ["chat", "classifier"] {
        for limits in [
            "",
            "context_len=9000",
            "max_output_tokens=20",
            "context_len=0\nmax_output_tokens=1",
            "context_len=-1\nmax_output_tokens=1",
            "context_len=10\nmax_output_tokens=10",
            "context_len='100'\nmax_output_tokens=1",
            "context_len=18446744073709551616\nmax_output_tokens=1",
        ] {
            fs::write(
                &persona,
                format!("{original}\n{}", connection(name, "override", limits)),
            )
            .unwrap();
            assert!(
                matches!(core.workspace(), Err(Error::Configuration { .. })),
                "{name}: {limits}"
            );
        }
    }
    fs::write(
        &persona,
        format!(
            "{original}\n{}{}",
            connection(
                "chat",
                "override-chat",
                "context_len=8000\nmax_output_tokens=90"
            ),
            connection(
                "classifier",
                "override-classifier",
                "context_len=4000\nmax_output_tokens=33"
            )
        ),
    )
    .unwrap();
    let p = core.workspace().unwrap().personas.remove(0);
    assert_eq!(
        p.context,
        ContextBudget {
            context_len: 8000,
            max_output_tokens: 90
        }
    );
    assert_eq!(
        p.classifier.unwrap().connection.budget(),
        ContextBudget {
            context_len: 4000,
            max_output_tokens: 33
        }
    );
    fs::write(&persona, &original).unwrap();
    for table in [
        "context",
        "defaults.context",
        "classifier_context",
        "defaults.classifier_context",
    ] {
        fs::write(
            &app,
            format!("{configured}\n[{table}]\ncontext_len=50\nmax_output_tokens=1\n"),
        )
        .unwrap();
        let error = core.workspace().unwrap_err().to_string();
        assert!(
            error.contains("Obsolete context table") && error.contains("move context_len"),
            "{error}"
        );
    }
    fs::write(&app, &configured).unwrap();
    fs::write(
        &persona,
        format!("{original}\n[context]\nmax_output_tokens=1"),
    )
    .unwrap();
    assert!(
        core.workspace()
            .unwrap_err()
            .to_string()
            .contains("Obsolete context table")
    );
}

fn with_limits(connection: &str, window: u64, output: u64) -> String {
    let mut doc: toml::Value = toml::from_str(connection).unwrap();
    doc["defaults"]["chat"]["context_len"] = toml::Value::Integer(window as i64);
    doc["defaults"]["chat"]["max_output_tokens"] = toml::Value::Integer(output as i64);
    toml::to_string(&doc).unwrap()
}

#[test]
fn both_protocols_use_configured_output_and_fail_context_before_http() {
    for protocol in ["chat_completions", "responses"] {
        let body = if protocol == "responses" {
            responses_support::success("accepted")
        } else {
            success("accepted")
        };
        let server = Server::new(vec![stream(body.clone()), stream(body)]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", protocol, "");
        let app = root.path().join("config.toml");
        let connection = fs::read_to_string(&app).unwrap();
        fs::write(&app, with_limits(&connection, 65536, 128)).unwrap();
        let (first, rx) = start(&core, &id, "accepted input");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let (_, body) = server.requests.recv().unwrap();
        assert_eq!(
            body[if protocol == "responses" {
                "max_output_tokens"
            } else {
                "max_tokens"
            }],
            128
        );
        let old = core.model_request(&first.id).unwrap();
        let accepted_state = core.response_settings(&id).unwrap();
        fs::write(&app, with_limits(&connection, 200, 100)).unwrap();
        let (failed, rx) = start(&core, &id, "input retained when context full");
        let last = terminal(&rx).pop().unwrap();
        assert_eq!(last.kind, ChatEventKind::Failed);
        assert!(last.text.contains("conservative input estimate"));
        assert!(last.text.contains("fresh context"));
        assert!(server.requests.try_recv().is_err());
        let saved = core.messages(&id).unwrap();
        assert_eq!(saved[2].content, "input retained when context full");
        assert_eq!(saved[3].status, MessageStatus::Failed);
        let failed_request = core.model_request(&failed.id).unwrap();
        assert_eq!(failed_request.session_id, old.session_id);
        assert_eq!(core.response_settings(&id).unwrap(), accepted_state);
        assert_eq!(failed_request.items[0].payload, old.items[0].payload);
        // A budget edit alone does not retire that new session or rewrite its prefix.
        fs::write(&app, with_limits(&connection, 65536, 128)).unwrap();
        let before = core.model_request(&failed.id).unwrap();
        assert_eq!(before.items[0].payload, failed_request.items[0].payload);
        assert_eq!(before.session_id, old.session_id);
        drop(first);
        drop(failed);
        drop(core);
        let core = reopen(root.path());
        core.fresh_context(&id).unwrap();
        let (next, rx) = start(&core, &id, "continue after deliberate fresh context");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let fresh = core.model_request(&next.id).unwrap();
        assert_ne!(fresh.session_id, old.session_id);
        assert_eq!(fresh.items.len(), 2);
        assert_eq!(core.messages(&id).unwrap().len(), 6);
        server.requests.recv().unwrap();
    }
}

#[test]
fn structured_repairs_keep_selected_output_cap_for_both_protocols() {
    for api in ["chat_completions", "responses"] {
        let response = |text: &str| {
            if api == "responses" {
                responses_support::candidate(text)
            } else {
                live_support::candidate(text)
            }
        };
        let server = Server::new(vec![
            stream(response("invalid candidate")),
            stream(response(&live_support::structured("accepted"))),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let path = root.path().join("config.toml");
        fs::write(
            &path,
            with_limits(&fs::read_to_string(&path).unwrap(), 65536, 123),
        )
        .unwrap();
        let (_task, rx) = start(&core, &id, "你好 repair");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        for attempt in 0..2 {
            let (_, body) = server.requests.recv().unwrap();
            assert_eq!(
                body[if api == "responses" {
                    "max_output_tokens"
                } else {
                    "max_tokens"
                }],
                123
            );
            assert!(body.get("context_len").is_none());
            assert_eq!(
                body[if api == "responses" {
                    "input"
                } else {
                    "messages"
                }]
                .as_array()
                .unwrap()
                .len(),
                2 + attempt
            );
        }
    }
}

#[test]
fn queued_work_retains_the_whole_connection_budget_accepted_before_manual_edits() {
    let mut first = stream(success("first"));
    first.delay_headers = true;
    let server = Server::new(vec![first, stream(success("queued"))]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let path = root.path().join("config.toml");
    let connection = fs::read_to_string(&path).unwrap();
    fs::write(&path, with_limits(&connection, 65536, 73)).unwrap();
    let (_first, first_rx) = start(&core, &id, "first");
    let (_, first_body) = server.requests.recv().unwrap();
    let (_queued, queued_rx) = start(&core, &id, "queued before limit edit");
    fs::write(&path, with_limits(&connection, 200, 11)).unwrap();
    assert_eq!(
        terminal(&first_rx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    assert_eq!(
        terminal(&queued_rx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    let (_, queued_body) = server.requests.recv().unwrap();
    assert_eq!(first_body["max_tokens"], 73);
    assert_eq!(queued_body["max_tokens"], 73);
    let (_new, new_rx) = start(&core, &id, "newly accepted small budget");
    assert_eq!(
        terminal(&new_rx).last().unwrap().kind,
        ChatEventKind::Failed
    );
    assert!(server.requests.try_recv().is_err());
}
