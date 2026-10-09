mod live_support;
mod responses_support;
use live_support::{Server, configure, isolated, reopen, start, stream, success, terminal};
use pebble_core::{ChatEventKind, ContextBudget, Error, MessageStatus};
use std::fs;

#[test]
fn config_defaults_and_field_inheritance_validate_output_reserve() {
    let (root, core, id) = isolated();
    let persona = core.workspace().unwrap().personas.remove(0);
    assert_eq!(persona.context, ContextBudget::default());
    fs::write(
        root.path().join("config.toml"),
        "[defaults.context]\ncontext_window_tokens=10000\nmax_output_tokens=500\n",
    )
    .unwrap();
    let path = std::path::Path::new(&persona.directory).join("config.toml");
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        format!("{original}\n[context]\nmax_output_tokens=200\n"),
    )
    .unwrap();
    let budget = core.workspace().unwrap().personas.remove(0).context;
    assert_eq!(
        budget,
        ContextBudget {
            context_window_tokens: 10000,
            max_output_tokens: 200
        }
    );
    for limits in [
        "context_window_tokens=0",
        "max_output_tokens=0",
        "max_output_tokens=10000",
        "context_window_tokens=-1",
        "context_window_tokens=18446744073709551616",
    ] {
        fs::write(&path, format!("{original}\n[context]\n{limits}\n")).unwrap();
        assert!(matches!(core.workspace(), Err(Error::Configuration { .. })));
    }
    assert!(!id.is_empty());
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
        fs::write(&app, format!("{connection}\n[defaults.context]\ncontext_window_tokens=65536\nmax_output_tokens=128\n")).unwrap();
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
        fs::write(&app, format!("{connection}\n[defaults.context]\ncontext_window_tokens=200\nmax_output_tokens=100\n")).unwrap();
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
        assert_eq!(failed_request.items[0].payload, old.items[0].payload);
        // A budget edit alone does not retire that new session or rewrite its prefix.
        fs::write(&app, format!("{connection}\n[defaults.context]\ncontext_window_tokens=65536\nmax_output_tokens=128\n")).unwrap();
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
