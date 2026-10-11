mod live_support;
mod responses_support;
use live_support::{
    Plan, Server, configure, isolated, reopen, retry, start, stream, success, terminal,
};
use pebble_core::{
    ChatEvent, ChatEventKind, ChatListener, CredentialChange, Error, MessageStatus, SettingsUpdate,
};
use serde_json::{Value, json};
use std::sync::Arc;
struct Sink;
impl ChatListener for Sink {
    fn on_event(&self, _: ChatEvent) -> Result<(), String> {
        Ok(())
    }
}
#[test]
fn both_protocols_retry_after_restart_reuse_frozen_input_and_keep_partial_attempts() {
    for protocol in ["chat_completions", "responses"] {
        let partial = if protocol == "responses" {
            responses_support::event(
                "response.output_text.delta",
                json!({"output_index":1,"content_index":0,"delta":"partial"}),
            )
        } else {
            live_support::event(json!({"content":"partial"}), Value::Null)
        };
        let accepted = if protocol == "responses" {
            responses_support::success("accepted")
        } else {
            success("accepted")
        };
        let server = Server::new(vec![stream(partial), stream(accepted)]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", protocol, "");
        let (original, rx) = start(&core, &id, "one original user");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Failed);
        let old = core.model_request(&original.id).unwrap();
        let old_reply = core.messages(&id).unwrap()[1].clone();
        assert!(old_reply.content.is_empty());
        assert!(old_reply.can_retry);
        assert!(!old_reply.failure_detail.is_empty());
        drop(original);
        drop(core);
        let core = reopen(root.path());
        assert_eq!(core.messages(&id).unwrap()[1], old_reply);
        let mut s = core.settings(None).unwrap();
        s.chat.max_output_tokens = Some(256);
        core.save_settings(
            None,
            &SettingsUpdate {
                chat: s.chat,
                credential: CredentialChange::Replace("repaired-secret".into()),
                persona: None,
            },
        )
        .unwrap();
        let (attempt, rx) = retry(&core, &id, &old_reply.id);
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let request = core.model_request(&attempt.id).unwrap();
        assert_eq!(request.session_id, old.session_id);
        assert_eq!(request.items.len(), old.items.len());
        for (a, b) in request.items.iter().zip(&old.items) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.payload, b.payload);
        }
        let replay = core.model_request(&old.call_id).unwrap();
        for (a, b) in old.items.iter().zip(replay.items) {
            assert_eq!(a.payload, b.payload);
        }
        let saved = core.messages(&id).unwrap();
        assert_eq!(saved.len(), 3);
        assert_eq!(saved[0].content, "one original user");
        assert!(saved[1].content.is_empty());
        assert_eq!(saved[1].status, MessageStatus::Failed);
        assert!(!saved[1].can_retry);
        assert_eq!(saved[2].status, MessageStatus::Complete);
        assert!(matches!(
            core.retry_message(&id, &old_reply.id, Arc::new(Sink)),
            Err(Error::RetryUnavailable(_))
        ));
        server.requests.recv().unwrap();
        let (headers, body) = server.requests.recv().unwrap();
        assert!(
            headers
                .to_lowercase()
                .contains("authorization: bearer repaired-secret")
        );
        assert_eq!(
            body[if protocol == "responses" {
                "max_output_tokens"
            } else {
                "max_tokens"
            }],
            256
        );
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM persona_events WHERE kind='received'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM persona_events WHERE kind='sent'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}
#[test]
fn queued_cancel_retry_admits_user_once_after_predecessor_native_output() {
    let server = Server::new(vec![
        Plan {
            status: 200,
            body: success("predecessor"),
            delay_headers: false,
            delay_tail: true,
        },
        stream(success("retried")),
        stream(success("followup")),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let (_first, rx) = start(&core, &id, "first user");
    server.requests.recv().unwrap();
    let (queued, qrx) = start(&core, &id, "queued unique user");
    queued.cancel();
    terminal(&qrx);
    let reply = core.messages(&id).unwrap()[3].id.clone();
    assert!(matches!(
        core.retry_message(&id, &reply, Arc::new(Sink)),
        Err(Error::Busy)
    ));
    terminal(&rx);
    let (attempt, arx) = retry(&core, &id, &reply);
    assert_eq!(
        terminal(&arx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    let request = core.model_request(&attempt.id).unwrap();
    assert_eq!(request.items.len(), 4);
    assert_eq!(
        serde_json::from_slice::<Value>(&request.items[2].payload).unwrap()["content"],
        live_support::structured("predecessor")
    );
    assert!(String::from_utf8_lossy(&request.items[3].payload).contains("queued unique user"));
    let (next, nrx) = start(&core, &id, "followup user");
    terminal(&nrx);
    let request = core.model_request(&next.id).unwrap();
    assert_eq!(
        request
            .items
            .iter()
            .filter(|i| String::from_utf8_lossy(&i.payload).contains("queued unique user"))
            .count(),
        1
    );
    assert_eq!(
        core.messages(&id)
            .unwrap()
            .iter()
            .filter(|m| m.sender_kind == "user")
            .count(),
        3
    );
}
#[test]
fn explicit_fresh_or_changed_protocol_retry_selects_only_original_user_and_followup_deduplicates() {
    for manual in [true, false] {
        let (root, core, id) = isolated();
        let (old, rx) = start(&core, &id, "old unrelated user");
        terminal(&rx);
        let (failed, frx) = start(&core, &id, "selected retry user");
        failed.cancel();
        terminal(&frx);
        let reply = core.messages(&id).unwrap().last().unwrap().id.clone();
        let server = Server::new(vec![
            stream(responses_support::success("retried")),
            stream(responses_support::success("followup")),
        ]);
        if manual {
            core.fresh_context(&id).unwrap();
        }
        configure(root.path(), &server.url, "mock", "responses", "");
        let (attempt, arx) = retry(&core, &id, &reply);
        assert_eq!(
            terminal(&arx).last().unwrap().kind,
            ChatEventKind::Completed
        );
        let request = core.model_request(&attempt.id).unwrap();
        assert_eq!(request.items.len(), 2);
        assert!(String::from_utf8_lossy(&request.items[1].payload).contains("selected retry user"));
        assert!(
            !request
                .items
                .iter()
                .any(|i| String::from_utf8_lossy(&i.payload).contains("old unrelated user"))
        );
        let (next, nrx) = start(&core, &id, "new followup");
        terminal(&nrx);
        let request = core.model_request(&next.id).unwrap();
        assert_eq!(
            request
                .items
                .iter()
                .filter(|i| String::from_utf8_lossy(&i.payload).contains("selected retry user"))
                .count(),
            1
        );
        assert!(!core.model_request(&old.id).unwrap().items.is_empty());
    }
}
