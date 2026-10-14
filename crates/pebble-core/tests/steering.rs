mod live_support;
mod responses_support;
use live_support::*;
use pebble_core::{ChatEvent, ChatEventKind, ChatListener, Core};
use std::sync::{Arc, mpsc};
use std::time::Duration;

struct Listener(mpsc::Sender<ChatEvent>);
impl ChatListener for Listener {
    fn on_event(&self, event: ChatEvent) -> Result<(), String> {
        self.0.send(event).map_err(|e| e.to_string())
    }
}
fn steer(
    core: &Core,
    room: &str,
    text: &str,
    mentions: &[String],
) -> (pebble_core::ChatSubmission, mpsc::Receiver<ChatEvent>) {
    let (tx, rx) = mpsc::channel();
    let submission = core
        .send_room_assigned_images(room, text, mentions, &[], false, Arc::new(Listener(tx)))
        .unwrap();
    (submission, rx)
}

#[test]
fn steering_replaces_inflight_output_and_preserves_all_receipts_both_protocols() {
    for api in ["chat_completions", "responses"] {
        let body = |text| {
            if api == "responses" {
                responses_support::candidate(&structured(text))
            } else {
                success(text)
            }
        };
        let mut held = stream(body("obsolete result"));
        held.delay_tail = true;
        let server = Server::new(vec![held, stream(body("updated result"))]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "steering-test", api, "");
        let room = core.workspace().unwrap().personas[0]
            .conversation_id
            .clone();
        let (first, first_rx) = steer(&core, &room, "original question", &[]);
        server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let (second, second_rx) = steer(&core, &room, "additional constraint", &[]);
        assert_eq!(
            terminal(&first_rx).last().unwrap().kind,
            ChatEventKind::Cancelled
        );
        assert_eq!(
            terminal(&second_rx).last().unwrap().kind,
            ChatEventKind::Completed
        );
        let messages = core.room_messages(&room).unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "original question");
        assert_eq!(messages[1].content, "additional constraint");
        assert_eq!(messages[2].content, "updated result");
        let request = core.model_request(&second.tasks[0].id).unwrap();
        let history = request
            .items
            .iter()
            .map(|i| String::from_utf8_lossy(&i.payload))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(history.contains("original question") && history.contains("additional constraint"));
        assert!(!history.contains("obsolete result"));
        drop(first);
        drop(second);
        drop(core);
        let reopened = reopen(root.path());
        assert_eq!(reopened.messages(&id).unwrap(), messages);
    }
}

#[test]
fn invalid_steering_does_not_cancel_the_original_reply() {
    let (root, core, _) = isolated();
    let room = core.workspace().unwrap().personas[0]
        .conversation_id
        .clone();
    let (first, rx) = steer(&core, &room, "keep this question", &[]);
    let (tx, _) = mpsc::channel();
    assert!(
        core.send_room_assigned_images(
            &room,
            "bad image",
            &[],
            &["missing-image".into()],
            false,
            Arc::new(Listener(tx))
        )
        .is_err()
    );
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    assert_eq!(core.room_messages(&room).unwrap().len(), 2);
    drop(first);
    drop(core);
    drop(root);
}

#[test]
fn rapid_group_steering_carries_required_speakers_and_keeps_other_rooms_running() {
    let (_root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap().id;
    let room = core.create_group("Group", &[a.clone(), b.clone()]).unwrap();
    let (private, private_rx) = start(&core, &b, "independent private question");
    let (first, _first_rx) = steer(
        &core,
        &room.id,
        "first group question",
        &[b.clone(), a.clone()],
    );
    let (second, _second_rx) = steer(&core, &room.id, "second group constraint", &[]);
    let (last, last_rx) = steer(&core, &room.id, "third group constraint", &[]);
    assert_eq!(
        terminal(&private_rx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    loop {
        let event = last_rx.recv_timeout(Duration::from_secs(8)).unwrap();
        if event.kind == ChatEventKind::GroupSettled {
            break;
        }
    }
    let messages = core.room_messages(&room.id).unwrap();
    assert_eq!(
        messages.iter().filter(|m| m.sender_kind == "user").count(),
        3
    );
    let replies = messages
        .iter()
        .filter(|m| m.sender_kind == "persona")
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 2);
    for reply in replies {
        assert_eq!(reply.status, pebble_core::MessageStatus::Complete);
        assert!(reply.content.contains("third group constraint"));
    }
    drop((first, second, last, private));
}
