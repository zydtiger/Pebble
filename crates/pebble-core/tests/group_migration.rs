mod group_support;
mod live_support;
mod migration_support;
use group_support::*;
use live_support::{isolated, reopen, start, terminal};

#[test]
fn populated_v012_upgrade_preserves_every_existing_identity_native_byte_state_and_association() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap();
    let (task, rx) = start(&core, &a, "populated private history");
    terminal(&rx);
    let (cancelled, rx) = start(&core, &b.id, "cancelled saved attempt");
    cancelled.cancel();
    terminal(&rx);
    let workspace = core.workspace().unwrap();
    let transcript_a = core.messages(&a).unwrap();
    let transcript_b = core.messages(&b.id).unwrap();
    let request = core.model_request(&task.id).unwrap();
    let state = core.response_settings(&a).unwrap();
    let db = db(root.path());
    let native: Vec<(String, String, String, Vec<u8>, i64)> = db
        .prepare("SELECT id,session_id,codec,payload,sequence FROM model_items ORDER BY id")
        .unwrap()
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let links: Vec<(String, String)> = db
        .prepare("SELECT item_id,event_id FROM model_item_events ORDER BY item_id,event_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop((task, cancelled));
    drop(core);
    std::thread::sleep(std::time::Duration::from_millis(20));
    migration_support::v7(root.path());
    assert_eq!(count(&db, "PRAGMA user_version"), 7);
    let core = reopen(root.path());
    let after = core.workspace().unwrap();
    assert_eq!(
        after
            .personas
            .iter()
            .map(|p| (&p.id, &p.conversation_id))
            .collect::<Vec<_>>(),
        workspace
            .personas
            .iter()
            .map(|p| (&p.id, &p.conversation_id))
            .collect::<Vec<_>>()
    );
    assert_eq!(core.messages(&a).unwrap(), transcript_a);
    assert_eq!(core.messages(&b.id).unwrap(), transcript_b);
    let migrated = core.model_request(&request.call_id).unwrap();
    assert_eq!(migrated.session_id, request.session_id);
    for (old, new) in request.items.iter().zip(migrated.items) {
        assert_eq!(old.id, new.id);
        assert_eq!(old.payload, new.payload);
    }
    assert_eq!(core.response_settings(&a).unwrap(), state);
    for (id, session, codec, payload, sequence) in native {
        let saved: (String, String, Vec<u8>, i64) = db
            .query_row(
                "SELECT session_id,codec,payload,sequence FROM model_items WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(saved, (session, codec, payload, sequence));
    }
    let saved_links: Vec<(String, String)> = db
        .prepare("SELECT item_id,event_id FROM model_item_events ORDER BY item_id,event_id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(links, saved_links);
    assert_eq!(count(&db, "PRAGMA user_version"), 8);
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM model_calls c JOIN conversations r ON r.id=c.conversation_id WHERE c.persona_id=r.persona_id AND c.persona_sequence=(SELECT sequence FROM messages WHERE id=c.reply_message_id)"
        ),
        2
    );
    let group = core
        .create_group("After upgrade", &[a.clone(), b.id])
        .unwrap();
    let (_turn, rx) = submit(&core, &group.id, "continue existing context", &[a]);
    finish(&rx, 1);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM conversations WHERE kind='direct'"
        ),
        2
    );
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}
