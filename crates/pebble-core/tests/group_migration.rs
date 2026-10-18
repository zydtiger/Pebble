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
    assert_eq!(count(&db, "PRAGMA user_version"), 21);
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

#[test]
fn populated_v013_additive_upgrade_preserves_group_memberships_and_publication_history() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap();
    let room = core
        .create_group("Existing group", &[a.clone(), b.id.clone()])
        .unwrap();
    let (post, rx) = submit(
        &core,
        &room.id,
        "Existing group observation",
        &[a.clone(), b.id],
    );
    finish(&rx, 2);
    let messages = core.room_messages(&room.id).unwrap();
    let state = core.response_settings(&a).unwrap();
    let request = core.model_request(&task(&post, 0).id).unwrap();
    let storage = db(root.path());
    let observations = count(&storage, "SELECT COUNT(*) FROM persona_events");
    let links = count(&storage, "SELECT COUNT(*) FROM model_item_events");
    let publications = count(
        &storage,
        "SELECT COUNT(*) FROM messages WHERE publication_sequence IS NOT NULL",
    );
    drop((post, core));
    std::thread::sleep(std::time::Duration::from_millis(30));
    migration_support::v9(root.path());
    storage
        .execute_batch("DROP TABLE participation_decisions; PRAGMA user_version=8;")
        .unwrap();
    let core = reopen(root.path());
    let restored_room = core
        .workspace()
        .unwrap()
        .rooms
        .into_iter()
        .find(|r| r.id == room.id)
        .unwrap();
    assert_eq!(
        (&restored_room.id, &restored_room.title, &restored_room.kind),
        (&room.id, &room.title, &room.kind)
    );
    assert_eq!(
        restored_room
            .members
            .iter()
            .map(|m| (&m.persona_id, &m.membership_id))
            .collect::<Vec<_>>(),
        room.members
            .iter()
            .map(|m| (&m.persona_id, &m.membership_id))
            .collect::<Vec<_>>()
    );
    assert_eq!(core.room_messages(&room.id).unwrap(), messages);
    assert_eq!(core.response_settings(&a).unwrap(), state);
    let restored = core.model_request(&request.call_id).unwrap();
    assert_eq!(restored.session_id, request.session_id);
    assert_eq!(
        restored
            .items
            .iter()
            .map(|i| (&i.id, &i.payload))
            .collect::<Vec<_>>(),
        request
            .items
            .iter()
            .map(|i| (&i.id, &i.payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        count(&storage, "SELECT COUNT(*) FROM persona_events"),
        observations
    );
    assert_eq!(
        count(&storage, "SELECT COUNT(*) FROM model_item_events"),
        links
    );
    assert_eq!(
        count(
            &storage,
            "SELECT COUNT(*) FROM messages WHERE publication_sequence IS NOT NULL"
        ),
        publications
    );
    assert_eq!(
        count(&storage, "SELECT COUNT(*) FROM participation_decisions"),
        0
    );
    assert_eq!(count(&storage, "PRAGMA user_version"), 21);
    assert!(
        !storage
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}

#[test]
fn populated_v014_upgrade_preserves_decisions_calls_native_bytes_and_contract_files() {
    let (root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap().id;
    let room = core
        .create_group("Existing v014 room", &[a.clone(), b])
        .unwrap();
    let (post, rx) = submit(
        &core,
        &room.id,
        "Preserved explicit request",
        std::slice::from_ref(&a),
    );
    finish(&rx, 1);
    let messages = core.room_messages(&room.id).unwrap();
    let request = core.model_request(&task(&post, 0).id).unwrap();
    let state = core.response_settings(&a).unwrap();
    let directory = core
        .workspace()
        .unwrap()
        .personas
        .into_iter()
        .find(|p| p.id == a)
        .unwrap()
        .directory;
    let files = [
        "config.toml",
        "response_schema.json",
        "SOUL.md",
        "MEMORY.md",
    ]
    .map(|f| std::fs::read(std::path::Path::new(&directory).join(f)).unwrap());
    let storage = db(root.path());
    let identities: Vec<(String, String, String, Option<String>)> = storage
        .prepare("SELECT id,message_id,persona_id,call_id FROM participation_decisions ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop((post, core));
    std::thread::sleep(std::time::Duration::from_millis(20));
    migration_support::v9(root.path());
    let core = reopen(root.path());
    assert_eq!(core.room_messages(&room.id).unwrap(), messages);
    assert_eq!(core.response_settings(&a).unwrap(), state);
    let restored = core.model_request(&request.call_id).unwrap();
    assert_eq!(
        request
            .items
            .iter()
            .map(|i| (&i.id, &i.payload))
            .collect::<Vec<_>>(),
        restored
            .items
            .iter()
            .map(|i| (&i.id, &i.payload))
            .collect::<Vec<_>>()
    );
    let after: Vec<(String, String, String, Option<String>)> = storage
        .prepare("SELECT id,message_id,persona_id,call_id FROM participation_decisions ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(identities, after);
    assert_eq!(
        files,
        [
            "config.toml",
            "response_schema.json",
            "SOUL.md",
            "MEMORY.md"
        ]
        .map(|f| std::fs::read(std::path::Path::new(&directory).join(f)).unwrap())
    );
    assert_eq!(count(&storage, "PRAGMA user_version"), 21);
    assert_eq!(count(&storage, "SELECT COUNT(*) FROM group_rounds"), 0);
    assert!(
        !storage
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}
