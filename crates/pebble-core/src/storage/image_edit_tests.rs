use super::{
    image_generation_tests::{finish, png, settings},
    *,
};
use crate::{
    Core, MessageStatus,
    image_generation::{
        ImageOperationKind, source,
        tool::{Arguments, Call, Size},
    },
};
use serde_json::{Value, json};

fn edit_call(id: &str) -> Call {
    Call {
        id: "edit-native".into(),
        arguments: Arguments {
            image_id: Some(id.into()),
            prompt: json!({"instruction":"Make the pebble green","preserve":["shape"]}).to_string(),
            backend: "studio".into(),
            size: Size {
                width: 256,
                height: 256,
            },
        },
    }
}
fn enqueue(
    core: &Core,
    persona: &crate::Persona,
    room: &str,
    ids: &[String],
) -> crate::history::Turn {
    let workspace = core.workspace().unwrap();
    let (user, mut turns, _) = core
        .database
        .enqueue_room_images(
            room,
            "Edit the image",
            if room == persona.conversation_id {
                &[]
            } else {
                std::slice::from_ref(&persona.id)
            },
            &workspace.personas,
            ids,
        )
        .unwrap();
    let turn = if room == persona.conversation_id {
        turns.remove(0)
    } else {
        match core
            .database
            .step_round(&user.id, &workspace.personas)
            .unwrap()
        {
            super::rounds::Step::Admit(turn, _) => *turn,
            _ => panic!("not admitted"),
        }
    };
    core.database.activate_turn(persona, &turn).unwrap()
}
fn prepare(
    core: &Core,
    persona: &crate::Persona,
    turn: &crate::history::Turn,
    source: &str,
) -> String {
    let request = core.database.request(&turn.call_id).unwrap();
    let (_, id) = core
        .database
        .prepare_image_generation(&request, &edit_call(source), &[], &settings(), "unused")
        .unwrap();
    assert!(id.is_some(), "expected accepted edit for {}", persona.id);
    id.unwrap()
}
fn complete(core: &Core, persona: &crate::Persona, id: &str, retry: bool) -> crate::Message {
    core.image_generation.import(id, &png()).unwrap();
    core.database
        .update_image_generation(id, |o| {
            o.status = "completed".into();
            Ok(())
        })
        .unwrap();
    let report = core.database.enqueue_image_report(id).unwrap().unwrap();
    let report = core.database.activate_turn(persona, &report).unwrap();
    let request = core.database.request(&report.call_id).unwrap();
    assert!(!request.image_generation_tools && !request.memory_tools && !request.delegation_tools);
    if retry {
        let failed = finish(core, &report, MessageStatus::Failed);
        let retry = core.database.enqueue_retry(persona, &failed.id).unwrap();
        let retry = core.database.activate_turn(persona, &retry).unwrap();
        finish(core, &retry, MessageStatus::Complete)
    } else {
        finish(core, &report, MessageStatus::Complete)
    }
}
#[test]
fn image_generation_edits_preserve_original_publish_once_and_support_successive_edits() {
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
        let original = core.import_image_data(&png()).unwrap();
        let turn = enqueue(
            &core,
            &persona,
            &persona.conversation_id,
            std::slice::from_ref(&original.id),
        );
        let id = prepare(&core, &persona, &turn, &original.id);
        finish(&core, &turn, MessageStatus::Failed);
        let published = complete(&core, &persona, &id, true);
        assert_eq!(published.images.len(), 1);
        assert_ne!(published.images[0].id, original.id);
        assert_eq!(std::fs::read(&original.source_path).unwrap(), png());
        let next = enqueue(&core, &persona, &persona.conversation_id, &[]);
        let next_id = prepare(&core, &persona, &next, &published.images[0].id);
        finish(&core, &next, MessageStatus::Failed);
        let edited = complete(&core, &persona, &next_id, false);
        assert_eq!(edited.images.len(), 1);
        assert_ne!(edited.images[0].id, published.images[0].id);
        assert_eq!(core.database.image_generations().unwrap().len(), 2);
        assert_eq!(
            core.database
                .image_generation(&next_id)
                .unwrap()
                .source
                .unwrap()
                .asset_id,
            published.images[0].id
        );
    }
}
#[test]
fn image_generation_source_access_rejects_drafts_other_rooms_prejoin_and_future_even_with_forged_parts()
 {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let owner = core.workspace().unwrap().personas.remove(0);
    let other = core.create_persona("Other").unwrap();
    let original = core.import_image_data(&png()).unwrap();
    let private = enqueue(
        &core,
        &owner,
        &owner.conversation_id,
        std::slice::from_ref(&original.id),
    );
    finish(&core, &private, MessageStatus::Complete);
    let group = core
        .create_group("source access", std::slice::from_ref(&owner.id))
        .unwrap();
    let frozen = enqueue(&core, &owner, &group.id, &[]);
    let request = core.database.request(&frozen.call_id).unwrap();
    // The source is in this persona's private native history, but not this room.
    let (result, op) = core
        .database
        .prepare_image_generation(
            &request,
            &edit_call(&original.id),
            &[],
            &settings(),
            "unused",
        )
        .unwrap();
    assert!(op.is_none());
    assert!(result.to_string().contains("current room"));
    finish(&core, &frozen, MessageStatus::Failed);
    let before_join = enqueue(&core, &owner, &group.id, std::slice::from_ref(&original.id));
    finish(&core, &before_join, MessageStatus::Complete);
    core.add_group_member(&group.id, &other.id).unwrap();
    let joined = enqueue(&core, &other, &group.id, &[]);
    let mut request = core.database.request(&joined.call_id).unwrap();
    let draft = core.import_image_data(&png()).unwrap();
    for id in [&original.id, &draft.id] {
        assert!(
            source::freeze(
                &core.database.connection.lock().unwrap(),
                &request,
                &other.id,
                id
            )
            .is_err()
        );
    }
    // Even a fabricated user image part cannot invent the frozen event association.
    let reference = {
        let db = core.database.connection.lock().unwrap();
        db.query_row(
            "SELECT reference FROM image_assets WHERE id=?1",
            [&original.id],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    let mut input: Value = serde_json::from_slice(&request.items.last().unwrap().payload).unwrap();
    input["content"] = json!([{ "type":"pebble_image_v1", "reference":serde_json::from_str::<Value>(&reference).unwrap() }]);
    request.items.last_mut().unwrap().payload = serde_json::to_vec(&input).unwrap();
    assert!(
        source::freeze(
            &core.database.connection.lock().unwrap(),
            &request,
            &other.id,
            &original.id
        )
        .is_err()
    );
    // This receipt was accepted after the request froze, so it cannot authorize editing.
    let workspace = core.workspace().unwrap();
    core.database
        .enqueue_room_images(
            &group.id,
            "future image",
            std::slice::from_ref(&other.id),
            &workspace.personas,
            std::slice::from_ref(&original.id),
        )
        .unwrap();
    assert!(
        source::freeze(
            &core.database.connection.lock().unwrap(),
            &request,
            &other.id,
            &original.id
        )
        .is_err()
    );
    assert!(core.database.image_generations().unwrap().is_empty());
}
#[test]
fn image_generation_generate_and_edit_share_canonical_trigger_claim_and_legacy_defaults() {
    for edit_first in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let persona = core.workspace().unwrap().personas.remove(0);
        let source = core.import_image_data(&png()).unwrap();
        let turn = enqueue(
            &core,
            &persona,
            &persona.conversation_id,
            std::slice::from_ref(&source.id),
        );
        let request = core.database.request(&turn.call_id).unwrap();
        let mut generate = edit_call(&source.id);
        generate.id = "generate-native".into();
        generate.arguments.image_id = None;
        generate.arguments.prompt = json!({"subject":"one pebble"}).to_string();
        let edit = edit_call(&source.id);
        let (first, second) = if edit_first {
            (&edit, &generate)
        } else {
            (&generate, &edit)
        };
        let (_, id) = core
            .database
            .prepare_image_generation(&request, first, &[], &settings(), "unused")
            .unwrap();
        let id = id.unwrap();
        let (result, dispatch) = core
            .database
            .prepare_image_generation(&request, second, &[], &settings(), "unused")
            .unwrap();
        assert!(dispatch.is_none());
        assert!(result.to_string().contains("existing"));
        assert_eq!(core.database.image_generations().unwrap().len(), 1);
        assert!(core.database.image_generation_used(&request).unwrap());
        if !edit_first {
            core.database.connection.lock().unwrap().execute("UPDATE image_generations SET record=json_remove(record,'$.kind','$.source','$.input_id','$.arguments.image_id') WHERE id=?1",[&id]).unwrap();
            let op = core.database.image_generation(&id).unwrap();
            assert_eq!(op.kind, ImageOperationKind::Generate);
            assert!(op.source.is_none() && op.input_id.is_none());
        }
    }
}
#[test]
fn image_generation_group_edits_publish_to_original_room_and_removal_prevents_late_publication() {
    for remove in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let owner = core.workspace().unwrap().personas.remove(0);
        let other = core.create_persona("Other").unwrap();
        let group = core
            .create_group("edit room", &[owner.id.clone(), other.id.clone()])
            .unwrap();
        let original = core.import_image_data(&png()).unwrap();
        let turn = enqueue(&core, &owner, &group.id, std::slice::from_ref(&original.id));
        let id = prepare(&core, &owner, &turn, &original.id);
        finish(&core, &turn, MessageStatus::Failed);
        core.image_generation.import(&id, &png()).unwrap();
        core.database
            .update_image_generation(&id, |o| {
                o.status = "completed".into();
                Ok(())
            })
            .unwrap();
        let report = core.database.enqueue_image_report(&id).unwrap().unwrap();
        let report = core.database.activate_turn(&owner, &report).unwrap();
        if remove {
            core.remove_group_member(&group.id, &owner.id).unwrap();
            core.add_group_member(&group.id, &owner.id).unwrap();
        }
        let published = finish(&core, &report, MessageStatus::Complete);
        assert_eq!(published.conversation_id, group.id);
        if remove {
            assert!(published.images.is_empty());
            assert!(core.database.image_generation(&id).unwrap().invalidated);
        } else {
            assert_eq!(published.images.len(), 1);
            assert_ne!(published.images[0].id, original.id);
            let db = core.database.connection.lock().unwrap();
            let n:i64=db.query_row("SELECT COUNT(*) FROM persona_events WHERE persona_id=?1 AND message_id=?2 AND kind='received'",params![other.id,published.id],|r|r.get(0)).unwrap();
            assert_eq!(n, 1);
            assert_eq!(
                super::images::references(&db, &published.id).unwrap()[0].asset_id,
                published.images[0].id
            );
        }
        assert_eq!(std::fs::read(&original.source_path).unwrap(), png());
    }
}

#[test]
fn image_generation_edit_can_use_previously_generated_public_pixels() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let (turn, id) =
        super::image_generation_tests::prepare(&core, &persona, &persona.conversation_id);
    finish(&core, &turn, MessageStatus::Failed);
    let published = complete(&core, &persona, &id, false);
    let source = published.images[0].id.clone();
    let edit = enqueue(&core, &persona, &persona.conversation_id, &[]);
    let edited_id = prepare(&core, &persona, &edit, &source);
    let op = core.database.image_generation(&edited_id).unwrap();
    assert_eq!(op.kind, ImageOperationKind::Edit);
    assert_eq!(op.source.unwrap().asset_id, source);
    finish(&core, &edit, MessageStatus::Failed);
    assert_ne!(
        complete(&core, &persona, &edited_id, false).images[0].id,
        source
    );
}
