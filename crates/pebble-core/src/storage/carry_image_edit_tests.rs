use super::*;
use crate::storage::rollover_tests::{complete as complete_chat, persona};

#[test]
fn image_generation_edit_accepts_only_frozen_current_room_carried_originals() {
    for api in ["chat_completions", "responses"] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let mut p = persona(&core, api);
        p.chat.as_mut().unwrap().context_len = 262144;
        p.context = p.chat.as_ref().unwrap().budget();
        let old = core.import_image_data(&png()).unwrap();
        let turn = enqueue(&core, &p, &p.conversation_id, std::slice::from_ref(&old.id));
        complete_chat(&core.database, &p, &turn, "old image");
        // Push this observed source outside the ten-message carry window.
        for _ in 0..5 {
            let turn = enqueue(&core, &p, &p.conversation_id, &[]);
            complete_chat(&core.database, &p, &turn, "later text");
        }
        let group = core
            .create_group("Other room", std::slice::from_ref(&p.id))
            .unwrap();
        let other_room = core.import_image_data(&png()).unwrap();
        let turn = enqueue(&core, &p, &group.id, std::slice::from_ref(&other_room.id));
        complete_chat(&core.database, &p, &turn, "group image");
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(1024, 800)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let original = core.import_image_data(bytes.get_ref()).unwrap();
        let turn = enqueue(
            &core,
            &p,
            &p.conversation_id,
            std::slice::from_ref(&original.id),
        );
        complete_chat(&core.database, &p, &turn, "uploaded original");
        let job = core.database.begin_context_maintenance(&p, true).unwrap();
        core.database.publish_maintenance(&job, &p).unwrap();
        let edit = enqueue(&core, &p, &p.conversation_id, &[]);
        let request = core.database.request(&edit.call_id).unwrap();
        assert!(
            request
                .items
                .iter()
                .all(|i| !String::from_utf8_lossy(&i.payload).contains(&original.id))
        );
        assert!(
            String::from_utf8_lossy(&request.recent_history.as_ref().unwrap().payload)
                .contains(&original.id)
        );
        // A genuinely exposed carried image from another room still cannot be edited here.
        assert!(
            source::freeze(
                &core.database.connection.lock().unwrap(),
                &request,
                &p.id,
                &other_room.id
            )
            .is_err()
        );
        let future = core.import_image_data(&png()).unwrap();
        core.database
            .enqueue_room_images(
                &p.conversation_id,
                "future image",
                &[],
                std::slice::from_ref(&p),
                std::slice::from_ref(&future.id),
            )
            .unwrap();
        for denied in [&old.id, &future.id] {
            let mut forged = request.clone();
            let reference =
                super::super::images::reference(&core.database.connection.lock().unwrap(), denied)
                    .unwrap();
            let recent = forged.recent_history.as_mut().unwrap();
            let mut native: Value = serde_json::from_slice(&recent.payload).unwrap();
            native["content"]
                .as_array_mut()
                .unwrap()
                .push(crate::images::reference_part(&reference));
            recent.payload = serde_json::to_vec(&native).unwrap();
            assert!(
                source::freeze(
                    &core.database.connection.lock().unwrap(),
                    &forged,
                    &p.id,
                    denied
                )
                .is_err()
            );
        }
        let source = source::freeze(
            &core.database.connection.lock().unwrap(),
            &request,
            &p.id,
            &original.id,
        )
        .unwrap();
        assert_eq!((source.width, source.height), (1024, 800));
        assert_eq!(source.read(root.path()).unwrap(), *bytes.get_ref());
        let operation = prepare(&core, &p, &edit, &original.id);
        let saved = core.database.image_generation(&operation).unwrap();
        assert_eq!(saved.kind, ImageOperationKind::Edit);
        assert_eq!(saved.source.unwrap().asset_id, original.id);
        drop(core);
        let core = Core::open(root.path()).unwrap();
        let restored = core.model_request(&edit.call_id).unwrap();
        assert_eq!(
            restored.recent_history.as_ref().unwrap().payload,
            request.recent_history.as_ref().unwrap().payload
        );
        let source = source::freeze(
            &core.database.connection.lock().unwrap(),
            &restored,
            &p.id,
            &original.id,
        )
        .unwrap();
        assert_eq!(source.read(root.path()).unwrap(), *bytes.get_ref());
        let saved = core.database.image_generation(&operation).unwrap();
        assert_eq!(saved.source.unwrap().asset_id, original.id);
    }
}
