use crate::{Core, MessageStatus, chat::RoomInput};

#[test]
fn group_steering_preserves_requested_order() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let a = core.workspace().unwrap().personas[0].id.clone();
    let b = core.create_persona("Kai").unwrap().id;
    let mut mentions = vec![a, b];
    mentions.sort();
    mentions.reverse();
    let room = core.create_group("Group", &mentions).unwrap();
    let personas = core.workspace().unwrap().personas;
    for picked in [&mentions[..], &[]] {
        let (_, _, decisions) = core
            .database
            .enqueue_input(
                &personas,
                RoomInput {
                    room: &room.id,
                    text: "Continue with this detail",
                    mentions: picked,
                    images: &[],
                    separate_tasks: false,
                    steer: true,
                },
            )
            .unwrap();
        assert_eq!(
            decisions
                .iter()
                .map(|d| d.decision.persona_id.clone())
                .collect::<Vec<_>>(),
            mentions
        );
    }
}

#[test]
fn independent_image_report_does_not_force_a_group_reply() {
    use crate::image_generation::tool::{Arguments, Call, Size};
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let a = core.workspace().unwrap().personas[0].id.clone();
    let b = core.create_persona("Kai").unwrap().id;
    let room = core.create_group("Group", &[a.clone(), b]).unwrap();
    let personas = core.workspace().unwrap().personas;
    let persona = personas.iter().find(|p| p.id == a).unwrap();
    let (user, _, _) = core
        .database
        .enqueue_input(
            &personas,
            RoomInput {
                room: &room.id,
                text: "Make an image",
                mentions: std::slice::from_ref(&a),
                images: &[],
                separate_tasks: false,
                steer: true,
            },
        )
        .unwrap();
    let super::rounds::Step::Admit(turn, _) =
        core.database.step_round(&user.id, &personas).unwrap()
    else {
        panic!("missing requested speaker");
    };
    let turn = core.database.activate_turn(persona, &turn).unwrap();
    let request = core.database.request(&turn.call_id).unwrap();
    let (_, image) = core
        .database
        .prepare_image_generation(
            &request,
            &Call {
                id: "synthetic-image".into(),
                arguments: Arguments {
                    prompt: "{\"subject\":\"pebble\"}".into(),
                    backend: "chatgpt".into(),
                    size: Size {
                        width: 256,
                        height: 256,
                    },
                },
            },
            &[],
            &crate::ImageGenerationSettings {
                studio: None,
                chatgpt: Some(crate::ChatGptImageSettings {
                    model: None,
                    effort: None,
                }),
            },
            "codex",
        )
        .unwrap();
    let image = image.unwrap();
    core.database
        .finish_turn(&turn, "", MessageStatus::Cancelled, None, None)
        .unwrap();
    core.database
        .update_image_generation(&image, |op| {
            op.status = "failed".into();
            Ok(())
        })
        .unwrap();
    let report = core.database.enqueue_image_report(&image).unwrap().unwrap();
    let (_, _, decisions) = core
        .database
        .enqueue_input(
            &personas,
            RoomInput {
                room: &room.id,
                text: "An unrelated observation",
                mentions: &[],
                images: &[],
                separate_tasks: false,
                steer: true,
            },
        )
        .unwrap();
    assert!(core.database.call_active(&report.call_id));
    assert!(decisions.iter().all(|d| d.decision.status != "mentioned"));
}

#[test]
fn unactivated_inputs_survive_steering_and_late_workers_cannot_publish() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let workspace = core.workspace().unwrap();
    let persona = &workspace.personas[0];
    let send = |text| {
        core.database
            .enqueue_input(
                &workspace.personas,
                RoomInput {
                    room: &persona.conversation_id,
                    text,
                    mentions: &[],
                    images: &[],
                    separate_tasks: false,
                    steer: true,
                },
            )
            .unwrap()
            .1
            .remove(0)
    };
    let first = send("unactivated original");
    let second = send("second constraint");
    let third = send("final constraint");
    assert!(core.database.activate_turn(persona, &first).is_err());
    core.database
        .finish_pending(&first, MessageStatus::Cancelled, None)
        .unwrap();
    core.database
        .finish_pending(&second, MessageStatus::Failed, Some("late failure"))
        .unwrap();
    let active = core.database.activate_turn(persona, &third).unwrap();
    let request = core.model_request(&active.call_id).unwrap();
    let history = request
        .items
        .iter()
        .map(|i| String::from_utf8_lossy(&i.payload))
        .collect::<Vec<_>>()
        .join("\n");
    for text in [
        "unactivated original",
        "second constraint",
        "final constraint",
    ] {
        assert!(history.contains(text), "Missing {text}");
    }
    let fourth = send("interrupt an activated request");
    core.database
        .finish_turn(
            &active,
            "obsolete completion",
            MessageStatus::Complete,
            None,
            None,
        )
        .unwrap();
    let visible = core.room_messages(&persona.conversation_id).unwrap();
    assert_eq!(visible.len(), 5);
    assert!(visible[..4].iter().all(|m| m.sender_kind == "user"));
    assert_eq!(visible[4].id, fourth.reply.id);
    assert!(!visible.iter().any(|m| m.content == "obsolete completion"));
}
