use serde_json::json;

use super::*;
use crate::{
    Core, ImageGenerationSettings, StudioSettings,
    image_generation::tool::{Arguments, Call, Size},
};
use image::{ImageFormat, RgbImage};
use std::io::Cursor;
fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    RgbImage::new(256, 256)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}
fn settings() -> ImageGenerationSettings {
    ImageGenerationSettings {
        studio: Some(StudioSettings {
            base_url: "https://api.example.com".into(),
            model_id: "model".into(),
            gpu_id: "gpu".into(),
            steps: None,
            guidance: None,
        }),
        chatgpt: None,
    }
}
fn prepare(core: &Core, persona: &crate::Persona, room: &str) -> (crate::history::Turn, String) {
    assert_eq!(room, persona.conversation_id);
    let turn = core
        .database
        .enqueue_turn(persona, "Generate one synthetic image")
        .unwrap();
    let turn = core.database.activate_turn(persona, &turn).unwrap();
    let request = core.database.request(&turn.call_id).unwrap();
    let call = Call {
        id: "native-image".into(),
        arguments: Arguments {
            prompt: json!({"subject":"synthetic pebble"}).to_string(),
            backend: "studio".into(),
            size: Size {
                width: 256,
                height: 256,
            },
        },
    };
    let (_, id) = core
        .database
        .prepare_image_generation(&request, &call, &[], &settings(), "codex")
        .unwrap();
    (turn, id.unwrap())
}
fn finish(core: &Core, turn: &crate::history::Turn, status: MessageStatus) -> crate::Message {
    let request = core.database.request(&turn.call_id).unwrap();
    let output = [
        json!({"role":"assistant","content":json!({"reply":"A synthetic image.","state":request.state_before}).to_string()}),
    ];
    core.database
        .finish_turn(
            turn,
            if status == MessageStatus::Complete {
                "A synthetic image."
            } else {
                ""
            },
            status,
            Some("synthetic fixture"),
            if status == MessageStatus::Complete {
                Some(&output)
            } else {
                None
            },
        )
        .unwrap()
}
#[test]
fn saved_pixels_publish_atomically_and_failed_report_retry_never_regenerates() {
    for api in ["chat_completions", "responses"] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let mut persona = core.workspace().unwrap().personas.remove(0);
        persona.chat = Some(crate::ChatConnection {
            model: "synthetic".into(),
            base_url: "https://api.example.com/v1".into(),
            api: api.into(),
            context_len: 65536,
            max_output_tokens: 512,
            api_key: None,
            api_key_env: None,
            responses_schema_format: None,
            request: Default::default(),
        });
        let (origin, id) = prepare(&core, &persona, &persona.conversation_id);
        finish(&core, &origin, MessageStatus::Failed);
        core.image_generation.import(&id, &png()).unwrap();
        core.database
            .update_image_generation(&id, |o| {
                o.status = "completed".into();
                Ok(())
            })
            .unwrap();
        let asset = core
            .database
            .image_generation(&id)
            .unwrap()
            .asset_id
            .unwrap();
        let queued = core.database.enqueue_image_report(&id).unwrap().unwrap();
        let report = core.database.activate_turn(&persona, &queued).unwrap();
        let request = core.database.request(&report.call_id).unwrap();
        assert!(
            !request.memory_tools && !request.delegation_tools && !request.image_generation_tools
        );
        let body = crate::live::request_body(persona.chat.as_ref().unwrap(), &request, 512)
            .unwrap()
            .1;
        assert!(body.to_string().contains("data:image/png;base64,"));
        assert!(
            core.database
                .messages(&persona.conversation_id)
                .unwrap()
                .iter()
                .all(|m| m.images.is_empty())
        );
        let failed = finish(&core, &report, MessageStatus::Failed);
        assert!(failed.images.is_empty());
        let retry = core.database.enqueue_retry(&persona, &failed.id).unwrap();
        let retry = core.database.activate_turn(&persona, &retry).unwrap();
        let published = finish(&core, &retry, MessageStatus::Complete);
        assert_eq!(published.images.len(), 1);
        assert_eq!(published.images[0].id, asset);
        assert_eq!(core.database.image_generations().unwrap().len(), 1);
        assert_eq!(
            core.database
                .image_generation(&id)
                .unwrap()
                .publication_status,
            "completed"
        );
    }
}
#[test]
fn stop_invalidates_saved_and_late_images_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (origin, id) = prepare(&core, &persona, &persona.conversation_id);
    finish(&core, &origin, MessageStatus::Failed);
    core.image_generation.import(&id, &png()).unwrap();
    core.database
        .update_image_generation(&id, |o| {
            o.status = "completed".into();
            Ok(())
        })
        .unwrap();
    let queued = core.database.enqueue_image_report(&id).unwrap().unwrap();
    let report = core.database.activate_turn(&persona, &queued).unwrap();
    core.stop_room(&persona.conversation_id).unwrap();
    let result = finish(&core, &report, MessageStatus::Complete);
    assert_ne!(result.status, MessageStatus::Complete);
    assert!(result.images.is_empty());
    assert!(core.database.image_generation(&id).unwrap().invalidated);
    assert!(core.database.enqueue_image_report(&id).unwrap().is_none());
}
#[test]
fn restart_preserves_remote_identity_frozen_config_and_marks_dispatch_uncertain() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (origin, id) = prepare(&core, &persona, &persona.conversation_id);
    finish(&core, &origin, MessageStatus::Failed);
    let legacy = "Original watercolor pebble; keep \"茶\".\nNo border.";
    core.database
        .update_image_generation(&id, |o| {
            // Existing records contain a prose string rather than serialized prompt JSON.
            o.arguments.prompt = legacy.into();
            o.dispatch_started = true;
            o.remote_id = Some("saved-run".into());
            Ok(())
        })
        .unwrap();
    // Inspect Database startup reconciliation without starting HTTP workers.
    drop(core);
    let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let db = loop {
        match Database::open(root.path()) {
            Ok(db) => break db,
            Err(crate::Error::AlreadyOpen) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(error) => panic!("{error}"),
        }
    };
    let op = db.image_generation(&id).unwrap();
    assert_eq!(op.status, "uncertain");
    assert_eq!(op.remote_id.as_deref(), Some("saved-run"));
    assert_eq!(op.arguments.prompt, legacy);
    assert_eq!(
        op.settings.studio.unwrap().base_url,
        "https://api.example.com"
    );
    assert!(op.asset_id.is_none());
}

#[test]
fn group_publication_fans_out_pixels_and_leave_rejoin_invalidates_original_interval() {
    for remove in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let owner = core.workspace().unwrap().personas.remove(0);
        let other = core.create_persona("Other").unwrap();
        let group = core
            .create_group("Synthetic images", &[owner.id.clone(), other.id.clone()])
            .unwrap();
        let workspace = core.workspace().unwrap();
        let (user, _, _) = core
            .database
            .enqueue_room(
                &group.id,
                "one synthetic image",
                std::slice::from_ref(&owner.id),
                &workspace.personas,
            )
            .unwrap();
        let round: String = core
            .database
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT id FROM group_rounds WHERE id=?1", [&user.id], |r| {
                r.get(0)
            })
            .unwrap();
        let origin = match core
            .database
            .step_round(&round, &workspace.personas)
            .unwrap()
        {
            super::rounds::Step::Admit(turn, _) => *turn,
            _ => panic!("Required image owner was not admitted"),
        };
        let origin = core.database.activate_turn(&owner, &origin).unwrap();
        let request = core.database.request(&origin.call_id).unwrap();
        let call = Call {
            id: "group-image".into(),
            arguments: Arguments {
                prompt: json!({"subject":"one synthetic pebble"}).to_string(),
                backend: "studio".into(),
                size: Size {
                    width: 256,
                    height: 256,
                },
            },
        };
        let (_, id) = core
            .database
            .prepare_image_generation(&request, &call, &[], &settings(), "codex")
            .unwrap();
        let id = id.unwrap();
        finish(&core, &origin, MessageStatus::Failed);
        core.image_generation.import(&id, &png()).unwrap();
        core.database
            .update_image_generation(&id, |o| {
                o.status = "completed".into();
                Ok(())
            })
            .unwrap();
        let queued = core.database.enqueue_image_report(&id).unwrap().unwrap();
        let report = core.database.activate_turn(&owner, &queued).unwrap();
        if remove {
            core.remove_group_member(&group.id, &owner.id).unwrap();
            core.add_group_member(&group.id, &owner.id).unwrap();
        }
        let result = finish(&core, &report, MessageStatus::Complete);
        if remove {
            assert_ne!(result.status, MessageStatus::Complete);
            assert!(result.images.is_empty());
        } else {
            assert_eq!(result.images.len(), 1);
            let db = core.database.connection.lock().unwrap();
            let received:i64=db.query_row("SELECT COUNT(*) FROM persona_events WHERE persona_id=?1 AND message_id=?2 AND kind='received'",params![other.id,result.id],|r|r.get(0)).unwrap();
            assert_eq!(received, 1);
            assert_eq!(
                super::images::references(&db, &result.id).unwrap()[0].asset_id,
                result.images[0].id
            );
            let self_receipts:i64=db.query_row("SELECT COUNT(*) FROM persona_events WHERE persona_id=?1 AND message_id=?2 AND kind='received'",params![owner.id,result.id],|r|r.get(0)).unwrap();
            assert_eq!(self_receipts, 0);
        }
    }
}
