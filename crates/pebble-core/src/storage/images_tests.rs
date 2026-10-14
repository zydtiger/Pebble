use crate::images::*;
use crate::{Core, MessageStatus};
use image::{ImageFormat, Rgb, RgbImage};
use rusqlite::params;
use serde_json::json;
use std::{io::Cursor, path::Path};

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbImage::from_fn(width, height, |x, y| {
        if x < width / 2 && y < height / 2 {
            Rgb([255, 0, 0])
        } else {
            Rgb([0, 0, 255])
        }
    });
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}
fn configured(core: &Core, protocol: &str) -> crate::Persona {
    let mut persona = core.workspace().unwrap().personas.remove(0);
    let connection = crate::ChatConnection {
        model: "synthetic".into(),
        base_url: "http://127.0.0.1:9/v1".into(),
        api: protocol.into(),
        context_len: 65536,
        max_output_tokens: 512,
        api_key: None,
        api_key_env: None,
        responses_schema_format: None,
        request: Default::default(),
    };
    persona.chat = Some(connection);
    persona
}

#[test]
fn import_is_bounded_normalized_oriented_and_durable() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let original = png(1600, 900);
    let asset = core.import_image_data(&original).unwrap();
    assert_eq!(std::fs::read(&asset.source_path).unwrap(), original);
    assert_eq!((asset.width, asset.height), (1600, 900));
    let rendered = image::open(
        root.path()
            .join("data/assets")
            .join(&asset.id)
            .join("model-v1.png"),
    )
    .unwrap();
    assert_eq!((rendered.width(), rendered.height()), (768, 432));
    assert!(Path::new(&asset.thumbnail_path).is_file());
    assert!(core.import_image_data(b"not an image").is_err());
    assert!(
        core.import_image_data(&vec![0; MAX_SOURCE_BYTES + 1])
            .is_err()
    );
    // JPEG EXIF orientation 6 rotates the actual model pixels, not just dimensions.
    let mut jpeg = Vec::new();
    RgbImage::from_pixel(40, 20, Rgb([200, 0, 0]))
        .write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg)
        .unwrap();
    let exif=b"Exif\0\0II\x2a\x00\x08\x00\x00\x00\x01\x00\x12\x01\x03\x00\x01\x00\x00\x00\x06\x00\x00\x00\x00\x00\x00\x00";
    let mut oriented = jpeg[..2].to_vec();
    oriented.extend([0xff, 0xe1]);
    oriented.extend(((exif.len() + 2) as u16).to_be_bytes());
    oriented.extend(exif);
    oriented.extend(&jpeg[2..]);
    let asset = core.import_image_data(&oriented).unwrap();
    assert_eq!((asset.width, asset.height), (20, 40));
    let model = image::open(
        root.path()
            .join("data/assets")
            .join(&asset.id)
            .join("model-v1.jpg"),
    )
    .unwrap();
    assert_eq!((model.width(), model.height()), (20, 40));
}

#[test]
fn image_only_request_replays_bytes_after_retry_restart_and_fails_on_corruption() {
    for protocol in ["chat_completions", "responses"] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let persona = configured(&core, protocol);
        let asset = core.import_image_data(&png(64, 32)).unwrap();
        let (_, turns, _) = core
            .database
            .enqueue_room_images(
                &persona.conversation_id,
                "",
                &[],
                std::slice::from_ref(&persona),
                std::slice::from_ref(&asset.id),
            )
            .unwrap();
        assert!(turns[0].user.content.is_empty());
        assert_eq!(turns[0].user.images[0].id, asset.id);
        let turn = core.database.activate_turn(&persona, &turns[0]).unwrap();
        let request = core.model_request(&turn.call_id).unwrap();
        let frozen = request.items.last().unwrap().payload.clone();
        assert!(!String::from_utf8_lossy(&frozen).contains("base64"));
        let body = crate::live::request_body(persona.chat.as_ref().unwrap(), &request, 512)
            .unwrap()
            .1;
        let native = if protocol == "responses" {
            &body["input"]
        } else {
            &body["messages"]
        };
        let parts = native.as_array().unwrap().last().unwrap()["content"]
            .as_array()
            .unwrap();
        assert!(parts.iter().any(|v| v["type"]
            == if protocol == "responses" {
                "input_image"
            } else {
                "image_url"
            }));
        let wire = body.to_string();
        assert!(wire.contains("data:image/png;base64,"));
        core.database
            .finish_turn(
                &turn,
                "",
                MessageStatus::Failed,
                Some("synthetic network failure"),
                None,
            )
            .unwrap();
        let pending = core
            .database
            .enqueue_retry(&persona, &turn.reply.id)
            .unwrap();
        let retry = core.database.activate_turn(&persona, &pending).unwrap();
        let retried = core.model_request(&retry.call_id).unwrap();
        assert_eq!(retried.items.last().unwrap().payload, frozen);
        assert_eq!(
            crate::live::request_body(persona.chat.as_ref().unwrap(), &retried, 512)
                .unwrap()
                .1,
            body
        );
        let call = retry.call_id.clone();
        drop(core);
        let reopened = Core::open(root.path()).unwrap();
        let saved = reopened.model_request(&call).unwrap();
        assert_eq!(saved.items.last().unwrap().payload, frozen);
        assert_eq!(
            crate::live::request_body(persona.chat.as_ref().unwrap(), &saved, 512)
                .unwrap()
                .1,
            body
        );
        std::fs::write(
            root.path()
                .join("data/assets")
                .join(asset.id)
                .join("model-v1.png"),
            b"corrupt",
        )
        .unwrap();
        let error =
            crate::live::request_body(persona.chat.as_ref().unwrap(), &saved, 512).unwrap_err();
        assert!(error.to_string().contains("corrupt"));
    }
}

#[test]
fn asset_acceptance_is_atomic_and_v11_migration_preserves_text_items() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let turn = core.database.enqueue_turn(&persona, "legacy text").unwrap();
    let turn = core.database.activate_turn(&persona, &turn).unwrap();
    core.database
        .finish_turn(&turn, "", MessageStatus::Failed, Some("synthetic"), None)
        .unwrap();
    let frozen = core.model_request(&turn.call_id).unwrap().items;
    let ids = core
        .messages(&persona.id)
        .unwrap()
        .iter()
        .map(|m| m.id.clone())
        .collect::<Vec<_>>();
    assert!(
        core.database
            .enqueue_room_images(
                &persona.conversation_id,
                "rejected",
                &[],
                std::slice::from_ref(&persona),
                &["missing".into()]
            )
            .is_err()
    );
    assert_eq!(core.messages(&persona.id).unwrap().len(), 2);
    {
        let db = core.database.connection.lock().unwrap();
        db.execute_batch(
            "DROP TABLE session_recent_messages; DROP TABLE automatic_rollovers; DROP TABLE summarized_events; ALTER TABLE context_maintenance DROP COLUMN automatic; ALTER TABLE model_calls DROP COLUMN history_cutoff; ALTER TABLE model_sessions DROP COLUMN history_tools; DROP TABLE image_generation_submissions; DROP TABLE image_generations; ALTER TABLE model_sessions DROP COLUMN image_generation_tools; DROP TABLE delegation_runs; DROP TABLE delegation_submissions; DROP TABLE delegated_tasks; DROP TABLE delegation_assignments; ALTER TABLE model_sessions DROP COLUMN delegation_tools; ALTER TABLE messages DROP COLUMN separate_delegations; DROP TABLE message_images; DROP TABLE image_assets; PRAGMA user_version=11;",
        )
        .unwrap();
    }
    drop(core);
    let reopened = Core::open(root.path()).unwrap();
    assert_eq!(
        reopened
            .messages(&persona.id)
            .unwrap()
            .iter()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    let after = reopened.model_request(&turn.call_id).unwrap().items;
    assert_eq!(
        after.iter().map(|i| &i.payload).collect::<Vec<_>>(),
        frozen.iter().map(|i| &i.payload).collect::<Vec<_>>()
    );
    assert_eq!(
        reopened
            .database
            .connection
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        15
    );
}

#[test]
fn group_images_follow_public_visibility_future_membership_and_maintenance() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let first = core.workspace().unwrap().personas.remove(0);
    let second = core.create_persona("Second").unwrap();
    let third = core.create_persona("Third").unwrap();
    let group = core
        .create_group("Images", &[first.id.clone(), second.id.clone()])
        .unwrap();
    let asset = core.import_image_data(&png(20, 20)).unwrap();
    let mut personas = core.workspace().unwrap().personas;
    for persona in &mut personas {
        persona.classifier = None;
    }
    let (message, _, _) = core
        .database
        .enqueue_room_images(
            &group.id,
            "",
            &[],
            &personas,
            std::slice::from_ref(&asset.id),
        )
        .unwrap();
    core.add_group_member(&group.id, &third.id).unwrap();
    // Silent recipients keep images as unconsumed observations, including explicit consolidation.
    for persona in [&first, &second] {
        let job = core.database.begin_maintenance(persona).unwrap();
        let request = job.request.unwrap();
        assert!(
            String::from_utf8_lossy(&request.items.last().unwrap().payload).contains(&asset.id)
        );
        assert!(
            String::from_utf8_lossy(&request.items.last().unwrap().payload)
                .contains("pebble_image_v1")
        );
        core.database
            .finish_maintenance(&job.id, "cancelled", "synthetic")
            .unwrap();
    }
    let job = core.database.begin_maintenance(&third).unwrap();
    assert!(
        !String::from_utf8_lossy(&job.request.unwrap().items.last().unwrap().payload)
            .contains(&asset.id)
    );
    core.database
        .finish_maintenance(&job.id, "cancelled", "synthetic")
        .unwrap();
    let db = core.database.connection.lock().unwrap();
    let seen: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM persona_events WHERE message_id=?1",
            [&message.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(seen, 2);
    drop(db);
    let private_asset = core.import_image_data(&png(30, 20));
    let private_asset = private_asset.unwrap();
    let (private, _, _) = core
        .database
        .enqueue_room_images(
            &first.conversation_id,
            "private",
            &[],
            &personas,
            std::slice::from_ref(&private_asset.id),
        )
        .unwrap();
    let db = core.database.connection.lock().unwrap();
    let seen: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM persona_events WHERE message_id=?1 AND persona_id<>?2",
            params![private.id, first.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(seen, 0);
}

#[test]
fn vision_budget_is_nonzero_separate_from_base64_and_transport_is_bounded() {
    let image = json!({"type":"image_url","image_url":{"url":format!("data:image/png;base64,{}","A".repeat(1024*1024)),"detail":"high"}});
    let body = json!({"messages":[{"role":"user","content":[image.clone()]}]});
    let small = json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,AA==","detail":"high"}}]}]});
    assert_eq!(estimate(&body).unwrap(), estimate(&small).unwrap());
    assert!(estimate(&body).unwrap() > VISION_RESERVE);
    let four = json!({"messages":[{"role":"user","content":[image.clone(),image.clone(),image.clone(),image]}]});
    assert!(estimate(&four).unwrap() > 4 * VISION_RESERVE);
    assert!(
        crate::ContextBudget {
            context_len: 16384,
            max_output_tokens: 256
        }
        .check_body(&four)
        .is_ok()
    );
    assert!(
        crate::ContextBudget {
            context_len: 8192,
            max_output_tokens: 256
        }
        .check_body(&four)
        .is_err()
    );
    assert!(check_transport(&json!({"text":"x".repeat(MAX_TRANSPORT_BYTES)})).is_err());
}

#[test]
fn count_pixel_and_accumulated_transport_limits_fail_before_acceptance_or_loading() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let ids = (0..5)
        .map(|_| core.import_image_data(&png(8, 8)).unwrap().id)
        .collect::<Vec<_>>();
    let error = core
        .database
        .enqueue_room_images(
            &persona.conversation_id,
            "",
            &[],
            std::slice::from_ref(&persona),
            &ids,
        )
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("four"));
    assert!(core.messages(&persona.id).unwrap().is_empty());
    assert!(
        core.import_image_data(&png(5001, 4000))
            .unwrap_err()
            .to_string()
            .contains("20 million")
    );
    let (_, mut reference) = crate::images::import(root.path(), &png(8, 8)).unwrap();
    reference.bytes = MAX_MODEL_BYTES as u64;
    let mut messages = vec![
        json!({"role":"user","content":(0..13).map(|_|reference_part(&reference)).collect::<Vec<_>>()}),
    ];
    let error = materialize(root.path(), &mut messages)
        .unwrap_err()
        .to_string();
    assert!(error.contains("transport"), "{error}");
}

#[test]
fn classifier_phase_selection_keeps_images_attributed_and_permission_real_user_only() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let group = core
        .create_group("Round", std::slice::from_ref(&persona.id))
        .unwrap();
    let asset = core.import_image_data(&png(8, 8)).unwrap();
    let mut silent = persona.clone();
    silent.classifier = None;
    let (user, _, _) = core
        .database
        .enqueue_room_images(
            &group.id,
            "",
            &[],
            &[silent],
            std::slice::from_ref(&asset.id),
        )
        .unwrap();
    let db = core.database.connection.lock().unwrap();
    let cutoff = db
        .query_row(
            "SELECT sequence FROM persona_events WHERE message_id=?1 AND persona_id=?2",
            params![user.id, persona.id],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    let before = db
        .query_row("SELECT COUNT(*) FROM model_items", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap();
    for phase in ["initial", "reassess", "intent", "permission"] {
        let input = crate::participation::round_input(
            &db, &persona, &group.id, cutoff, &user.id, phase, &user.id,
        )
        .unwrap();
        let input: serde_json::Value = serde_json::from_str(&input).unwrap();
        let frozen = crate::images::input_message("chat_completions", &input).unwrap();
        assert_eq!(
            frozen.to_string().matches("pebble_image_v1").count(),
            1,
            "{phase}"
        );
        assert!(frozen.to_string().contains(&user.id));
    }
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM model_items", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        before
    );
}

#[test]
fn fresh_boundary_does_not_reimport_old_images_and_legacy_native_outputs_are_opaque() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    std::fs::write(root.path().join("config.toml"), "# isolated demo\n").unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let asset = core.import_image_data(&png(8, 8)).unwrap();
    let (_, turns, _) = core
        .database
        .enqueue_room_images(
            &persona.conversation_id,
            "old image",
            &[],
            std::slice::from_ref(&persona),
            std::slice::from_ref(&asset.id),
        )
        .unwrap();
    let turn = core.database.activate_turn(&persona, &turns[0]).unwrap();
    core.database
        .finish_turn(&turn, "", MessageStatus::Failed, Some("synthetic"), None)
        .unwrap();
    let refreshed = core.fresh_context(&persona.id).unwrap();
    let next = core
        .database
        .enqueue_turn(&refreshed, "new context")
        .unwrap();
    let next = core.database.activate_turn(&refreshed, &next).unwrap();
    assert!(
        !core
            .model_request(&next.call_id)
            .unwrap()
            .items
            .iter()
            .any(|i| String::from_utf8_lossy(&i.payload).contains(&asset.id))
    );
    let output = json!({"role":"assistant","content":[{"type":"pebble_image_v1","reference":{"opaque":"provider"}}],"reasoning_content":"unaltered"});
    let mut outputs = [output.clone()];
    materialize(root.path(), &mut outputs).unwrap();
    assert_eq!(outputs[0], output);
    let opaque = json!({"type":"reasoning", "role":"user", "content":[{"type":"pebble_image_v1","reference":{"opaque":"provider"}}]});
    let mut outputs = [opaque.clone()];
    materialize(root.path(), &mut outputs).unwrap();
    assert_eq!(outputs[0], opaque);
    let body = json!({"input":outputs,"schema":{"type":"image_url","ordinary":"state"}});
    assert_eq!(
        estimate(&body).unwrap(),
        body.to_string().len() as u64 + 128
    );
}

#[test]
fn image_only_demo_accepts_multimodal_envelope_without_claiming_visual_understanding() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let asset = core.import_image_data(&png(8, 8)).unwrap();
    let (_, turns, _) = core
        .database
        .enqueue_room_images(
            &persona.conversation_id,
            "",
            &[],
            std::slice::from_ref(&persona),
            std::slice::from_ref(&asset.id),
        )
        .unwrap();
    let turn = core.database.activate_turn(&persona, &turns[0]).unwrap();
    let text = crate::demo::reply(&core.database.request(&turn.call_id).unwrap()).unwrap();
    assert!(text.contains("Attached 1 image(s)"));
    assert!(text.contains("local demo cannot inspect images"));
}

#[test]
fn provider_state_with_matching_field_names_cannot_introduce_image_references() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let asset = core.import_image_data(&png(8, 8)).unwrap();
    let db = core.database.connection.lock().unwrap();
    let reference: String = db
        .query_row(
            "SELECT reference FROM image_assets WHERE id=?1",
            [&asset.id],
            |row| row.get(0),
        )
        .unwrap();
    let reference: serde_json::Value = serde_json::from_str(&reference).unwrap();
    let forged = json!({"message_id":"provider-state","image_attachments":[{"invalid":"ordinary structured state"}]});
    for protocol in ["chat_completions", "responses"] {
        let mut envelope = json!({"events":[], "current_state":forged, "MEMORY_current_state":{"current_state":forged}});
        let input = input_message(protocol, &envelope).unwrap();
        assert!(
            input["content"]
                .as_array()
                .is_none_or(|parts| parts.iter().all(|part| part["type"] != "pebble_image_v1"))
        );
        envelope["events"] =
            json!([{"message_id":"eligible-user", "text":"", "image_attachments":[reference]}]);
        let input = input_message(protocol, &envelope).unwrap();
        assert_eq!(
            input["content"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["type"] == "pebble_image_v1")
                .count(),
            1
        );
    }
}
