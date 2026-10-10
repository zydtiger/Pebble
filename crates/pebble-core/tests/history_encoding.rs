mod live_support;
mod migration_support;
mod responses_support;
use live_support::{Server, configure, isolated, reopen, start, stream, success, terminal};
use pebble_core::{ChatEventKind, Error};
use rusqlite::params;

#[test]
fn mixed_codec_native_history_restarts_and_replays_exact_bytes_for_both_protocols() {
    for protocol in ["chat_completions", "responses"] {
        let opaque = "opaque encrypted state 🪨".repeat(500);
        let body = if protocol == "responses" {
            let mut output = responses_support::items("first");
            output[0]["encrypted_content"] = opaque.clone().into();
            responses_support::event(
                "response.output_text.delta",
                serde_json::json!({"output_index":1,"content_index":0,"delta":live_support::structured("first")}),
            ) + &responses_support::completed(&output)
        } else {
            live_support::event(
                serde_json::json!({"content":live_support::structured("first"), "reasoning_content":opaque}),
                serde_json::Value::Null,
            ) + &live_support::event(serde_json::json!({}), serde_json::json!("stop"))
                + "data: [DONE]\n\n"
        };
        let second_body = if protocol == "responses" {
            responses_support::success("second")
        } else {
            success("second")
        };
        let server = Server::new(vec![stream(body), stream(second_body)]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", protocol, "");
        let (first, rx) = start(&core, &id, "first input");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let frozen = core.model_request(&first.id).unwrap();
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        let sizes: (i64, i64) = db
            .query_row(
                "SELECT decoded_length,length(payload) FROM model_items WHERE codec='zstd'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        println!(
            "Synthetic {protocol} native opaque item: decoded={} stored={} bytes",
            sizes.0, sizes.1
        );
        assert!(sizes.1 < sizes.0);
        assert!(
            db.query_row(
                "SELECT COUNT(*) FROM model_items WHERE codec='json'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap()
                > 0
        );
        drop(first);
        drop(core);
        let core = reopen(root.path());
        let (second, rx) = start(&core, &id, "second input");
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let next = core.model_request(&second.id).unwrap();
        for (a, b) in frozen.items.iter().zip(&next.items) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.payload, b.payload);
        }
        assert!(
            next.items
                .iter()
                .any(|i| String::from_utf8_lossy(&i.payload).contains(&opaque))
        );
        let replay = core.model_request(&second.id).unwrap();
        for (a, b) in next.items.iter().zip(replay.items) {
            assert_eq!(a.payload, b.payload);
        }
    }
}

#[test]
fn schema4_raw_history_migration_preserves_ids_and_exact_json_bytes() {
    let (root, core, id) = isolated();
    let (task, rx) = start(&core, &id, "native raw migration 🪨");
    terminal(&rx);
    let before = core.model_request(&task.id).unwrap();
    let messages = core.messages(&id).unwrap();
    drop(task);
    drop(core);
    let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    let rows = db
        .prepare("SELECT id,codec,payload FROM model_items")
        .unwrap()
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for (id, codec, bytes) in rows {
        let raw = if codec == "zstd" {
            zstd::stream::decode_all(std::io::Cursor::new(bytes)).unwrap()
        } else {
            bytes
        };
        db.execute(
            "UPDATE model_items SET codec='json',payload=?2 WHERE id=?1",
            params![id, raw],
        )
        .unwrap();
    }
    migration_support::v7(root.path());
    db.execute_batch("DROP TABLE persona_states; ALTER TABLE model_sessions DROP COLUMN response_schema; ALTER TABLE model_calls DROP COLUMN state_before; ALTER TABLE model_calls DROP COLUMN validation_errors; ALTER TABLE model_calls DROP COLUMN retry_of; ALTER TABLE model_items DROP COLUMN decoded_length; ALTER TABLE model_sessions DROP COLUMN context_reason; PRAGMA user_version=4;").unwrap();
    let core = reopen(root.path());
    assert_eq!(core.messages(&id).unwrap(), messages);
    let after = core.model_request(&before.call_id).unwrap();
    assert_eq!(before.session_id, after.session_id);
    for (a, b) in before.items.iter().zip(after.items) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.payload, b.payload);
    }
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        9
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM model_items WHERE codec!='json'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert!(
        !db.prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}

#[test]
fn request_loader_preflights_total_size_and_rejects_corrupt_or_lying_codec_metadata() {
    for fault in ["total", "codec", "length", "truncated", "lying"] {
        let (root, core, id) = isolated();
        let (task, rx) = start(&core, &id, "saved input");
        terminal(&rx);
        let request = core.model_request(&task.id).unwrap();
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        db.execute_batch("PRAGMA ignore_check_constraints=ON;")
            .unwrap();
        match fault {
            "total" => {
                db.execute("UPDATE model_items SET codec='zstd',decoded_length=4300000,payload=x'00' WHERE session_id=?1",[&request.session_id]).unwrap();
            }
            "codec" => {
                db.execute(
                    "UPDATE model_items SET codec='unknown' WHERE id=?1",
                    [&request.items[0].id],
                )
                .unwrap();
            }
            "length" => {
                db.execute(
                    "UPDATE model_items SET decoded_length=1 WHERE id=?1",
                    [&request.items[0].id],
                )
                .unwrap();
            }
            "truncated" | "lying" => {
                let raw = vec![
                    b'x';
                    if fault == "lying" {
                        9 * 1024 * 1024
                    } else {
                        4000
                    }
                ];
                let mut bytes = zstd::stream::encode_all(std::io::Cursor::new(raw), 3).unwrap();
                let length = if fault == "lying" {
                    1
                } else {
                    bytes.pop();
                    4000
                };
                db.execute(
                    "UPDATE model_items SET codec='zstd',decoded_length=?2,payload=?3 WHERE id=?1",
                    params![request.items[0].id, length, bytes],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let failure = core.model_request(&task.id).unwrap_err();
        if fault == "total" {
            assert!(matches!(failure, Error::ContextLimit(_)));
        } else {
            assert!(matches!(failure, Error::History(_)));
        }
        assert!(failure.to_string().contains("fresh context"));
        // Explicit fresh context recovers without decoding or deleting old corrupt items.
        core.fresh_context(&id).unwrap();
        assert_eq!(core.messages(&id).unwrap().len(), 2);
    }
}
