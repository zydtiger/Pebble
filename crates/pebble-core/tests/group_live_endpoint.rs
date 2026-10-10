mod group_support;
mod live_support;
use group_support::*;
use live_support::*;
use pebble_core::{ChatEventKind, CredentialChange, SettingsUpdate};
use std::io::Write;

#[test]
#[ignore = "requires an explicitly selected endpoint; isolated synthetic groups only"]
fn opt_in_live_groups_both_protocols() {
    let url = std::env::var("PEBBLE_LIVE_API_ROOT").expect("set PEBBLE_LIVE_API_ROOT");
    let model = std::env::var("PEBBLE_LIVE_MODEL").expect("set PEBBLE_LIVE_MODEL");
    for api in ["chat_completions", "responses"] {
        let (root, core, a) = isolated();
        let b = core.create_persona("Synthetic observer").unwrap();
        let outsider = core.create_persona("Synthetic nonmember").unwrap();
        configure(root.path(), &url, &model, api, "");
        if api == "responses" {
            let format = std::env::var("PEBBLE_LIVE_RESPONSES_SCHEMA_FORMAT")
                .unwrap_or("text_format".into());
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(root.path().join("config.toml"))
                    .unwrap(),
                "responses_schema_format={format:?}"
            )
            .unwrap();
        }
        let settings = core.settings(None).unwrap();
        core.save_settings(
            None,
            &SettingsUpdate {
                chat: settings.chat,
                credential: CredentialChange::Keep,
                context_window_tokens: Some(65536),
                max_output_tokens: Some(1024),
                persona: None,
            },
        )
        .unwrap();
        let (private, rx) = start(
            &core,
            &a,
            "Synthetic private token PRIVATE_SYNTHETIC_013. Reply with exactly PEBBLE_PRIVATE_OK.",
        );
        let events = terminal(&rx);
        assert_eq!(
            events.last().unwrap().kind,
            ChatEventKind::Completed,
            "{}",
            events.last().unwrap().text
        );
        let first = core.model_request(&private.id).unwrap();
        let group = core
            .create_group("Synthetic live group", &[a.clone(), b.id.clone()])
            .unwrap();
        let (silent, _) = submit(&core, &group.id, "PUBLIC_SYNTHETIC_LISTEN_ONLY", &[]);
        assert!(silent.tasks.is_empty());
        let (public, rx) = submit(
            &core,
            &group.id,
            "Reply with exactly PEBBLE_GROUP_OK. This is a synthetic public group request.",
            std::slice::from_ref(&a),
        );
        let events = finish(&rx, 1);
        assert_eq!(
            events.last().unwrap().kind,
            ChatEventKind::Completed,
            "{}",
            events.last().unwrap().text
        );
        assert!(events.iter().all(|e| e.kind != ChatEventKind::TextDelta));
        let group_request = core.model_request(&public.tasks[0].id).unwrap();
        assert_eq!(group_request.session_id, first.session_id);
        assert_eq!(
            envelope(&group_request)["reply_request"]["conversation_id"],
            group.id
        );
        for (old, new) in first.items.iter().zip(&group_request.items) {
            assert_eq!(old.id, new.id);
            assert_eq!(old.payload, new.payload);
        }
        let saved = core.room_messages(&group.id).unwrap();
        assert!(saved.last().unwrap().content.contains("PEBBLE_GROUP_OK"));
        let (observer, rx) = start(&core, &b.id, "Reply with exactly PEBBLE_OBSERVER_OK.");
        let events = terminal(&rx);
        assert_eq!(
            events.last().unwrap().kind,
            ChatEventKind::Completed,
            "{}",
            events.last().unwrap().text
        );
        let request = core.model_request(&observer.id).unwrap();
        let input = envelope(&request);
        assert_eq!(
            input["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["message_id"] == saved.last().unwrap().id)
                .count(),
            1
        );
        assert!(!native(&request).contains("PRIVATE_SYNTHETIC_013"));
        let (continued, rx) = start(&core, &a, "Reply with exactly PEBBLE_CONTINUED_OK.");
        let events = terminal(&rx);
        assert_eq!(
            events.last().unwrap().kind,
            ChatEventKind::Completed,
            "{}",
            events.last().unwrap().text
        );
        let request = core.model_request(&continued.id).unwrap();
        assert_eq!(request.session_id, first.session_id);
        assert!(native(&request).contains("PRIVATE_SYNTHETIC_013"));
        assert!(native(&request).contains("PEBBLE_GROUP_OK"));
        let db = db(root.path());
        let delivered: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM persona_events WHERE persona_id=?1 AND conversation_id=?2",
                rusqlite::params![outsider.id, group.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(delivered, 0);
        assert!(
            !db.prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
        println!(
            "{api}: private→group→private shared native session; silent member public observations exactly once; nonmember isolation; validated reply {:?}; synthetic temporary data only",
            saved.last().unwrap().content
        );
    }
}
