mod live_support;
mod responses_support;
use live_support::{Server, configure, isolated, stream, terminal};
use pebble_core::{ChatEvent, ChatEventKind, ChatListener};
use serde_json::Value;
use std::{
    io::Cursor,
    sync::{Arc, mpsc},
};
struct Listener(mpsc::Sender<ChatEvent>);
impl ChatListener for Listener {
    fn on_event(&self, event: ChatEvent) -> Result<(), String> {
        self.0.send(event).map_err(|e| e.to_string())
    }
}
fn image(color: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbImage::from_pixel(64, 64, image::Rgb(color))
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}
fn inputs<'a>(body: &'a Value, protocol: &str) -> &'a Value {
    &body[if protocol == "responses" {
        "input"
    } else {
        "messages"
    }]
}
fn image_urls(body: &Value, protocol: &str) -> Vec<String> {
    inputs(body, protocol)
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "user")
        .flat_map(|m| m["content"].as_array().into_iter().flatten())
        .filter_map(|p| {
            if protocol == "responses" {
                p["image_url"].as_str()
            } else {
                p["image_url"]["url"].as_str()
            }
        })
        .map(str::to_owned)
        .collect()
}
#[test]
fn native_adapters_send_real_ordered_pixels_and_retry_reuses_exact_frozen_assets() {
    use base64::Engine;
    for protocol in ["chat_completions", "responses"] {
        let output = if protocol == "responses" {
            responses_support::success("I see two colors")
        } else {
            live_support::success("I see two colors")
        };
        let server = Server::new(vec![
            live_support::Plan {
                status: 503,
                body: "synthetic failure".into(),
                delay_headers: false,
                delay_tail: false,
            },
            stream(output),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "vision", protocol, "");
        let persona = core
            .workspace()
            .unwrap()
            .personas
            .into_iter()
            .find(|p| p.id == id)
            .unwrap();
        let assets = [
            core.import_image_data(&image([255, 0, 0])).unwrap(),
            core.import_image_data(&image([0, 0, 255])).unwrap(),
        ];
        let (tx, rx) = mpsc::channel();
        let submission = core
            .send_room_images(
                &persona.conversation_id,
                "Compare these images",
                &[],
                &assets.iter().map(|a| a.id.clone()).collect::<Vec<_>>(),
                Arc::new(Listener(tx)),
            )
            .unwrap();
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Failed);
        let (_, first) = server
            .requests
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        let urls = image_urls(&first, protocol);
        assert_eq!(urls.len(), 2);
        for (url, expected) in urls.iter().zip([[255, 0, 0], [0, 0, 255]]) {
            let encoded = url.split_once(',').unwrap().1;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap();
            assert_eq!(
                image::load_from_memory(&decoded)
                    .unwrap()
                    .to_rgb8()
                    .get_pixel(30, 30)
                    .0,
                expected
            );
        }
        let task = &submission.tasks[0];
        let frozen = core.model_request(&task.id).unwrap();
        assert!(
            frozen
                .items
                .iter()
                .all(|i| !String::from_utf8_lossy(&i.payload).contains("base64"))
        );
        let reply = task.messages.last().unwrap().id.clone();
        drop(submission);
        drop(core);
        let core = pebble_core::Core::open(root.path()).unwrap();
        let (tx, rx) = mpsc::channel();
        let retried = core
            .retry_message(&id, &reply, Arc::new(Listener(tx)))
            .unwrap();
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let (_, second) = server
            .requests
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(image_urls(&second, protocol), urls);
        assert_eq!(
            core.model_request(&retried.id)
                .unwrap()
                .items
                .last()
                .unwrap()
                .payload,
            frozen.items.last().unwrap().payload
        );
    }
}

#[test]
#[ignore = "explicit configured vision service; synthetic pixels and isolated root only"]
fn opt_in_selected_chat_protocols_consume_changed_pixels() {
    let url = std::env::var("PEBBLE_LIVE_API_ROOT").expect("PEBBLE_LIVE_API_ROOT");
    let model = std::env::var("PEBBLE_LIVE_MODEL").expect("PEBBLE_LIVE_MODEL");
    for protocol in ["chat_completions", "responses"] {
        let (root, core, id) = isolated();
        configure(root.path(), &url, &model, protocol, "");
        if protocol == "responses" {
            let path = root.path().join("config.toml");
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                path,
                format!("{text}responses_schema_format=\"response_format\"\n"),
            )
            .unwrap();
        }
        let persona = core
            .workspace()
            .unwrap()
            .personas
            .into_iter()
            .find(|p| p.id == id)
            .unwrap();
        for (color, label) in [([255, 0, 0], "red"), ([0, 0, 255], "blue")] {
            let asset = core.import_image_data(&image(color)).unwrap();
            let (tx, rx) = mpsc::channel();
            let post=core.send_room_images(&persona.conversation_id,"Name the dominant color of the image attached to THIS latest message. Reply with only the English color word. Keep state unchanged.",&[],&[asset.id],Arc::new(Listener(tx))).unwrap();
            let events = terminal(&rx);
            let last = events.last().unwrap();
            assert_eq!(
                last.kind,
                ChatEventKind::Completed,
                "{protocol}: {}",
                last.text
            );
            let text = core
                .room_messages(&persona.conversation_id)
                .unwrap()
                .last()
                .unwrap()
                .content
                .to_lowercase();
            assert!(text.contains(label), "{protocol} expected {label}: {text}");
            println!(
                "{protocol}: changed synthetic pixels => {label}; accepted {}",
                post.tasks[0].id
            );
        }
    }
}

#[test]
fn consolidation_receives_silent_image_then_fresh_chat_excludes_old_pixels() {
    use serde_json::json;
    for protocol in ["chat_completions", "responses"] {
        let tool = if protocol == "responses" {
            format!(
                "data: {}\n\n",
                json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","id":"fc-memory-vision","status":"completed","call_id":"memory-vision","name":"update_memory","arguments":json!({"content":"Synthetic visual memory: a red circle."}).to_string()}]}})
            )
        } else {
            format!(
                "{}{}data: [DONE]\n\n",
                live_support::event(
                    json!({"tool_calls":[{"index":0,"id":"memory-vision","type":"function","function":{"name":"update_memory","arguments":json!({"content":"Synthetic visual memory: a red circle."}).to_string()}}]}),
                    Value::Null
                ),
                live_support::event(json!({}), json!("tool_calls"))
            )
        };
        let output = if protocol == "responses" {
            responses_support::success("Fresh context")
        } else {
            live_support::success("Fresh context")
        };
        let server = Server::memory(vec![stream(tool), stream(output)]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "vision", protocol, "");
        let persona = core.workspace().unwrap().personas.remove(0);
        let group = core
            .create_group("Silent image", std::slice::from_ref(&id))
            .unwrap();
        let asset = core.import_image_data(&image([255, 0, 0])).unwrap();
        let (tx, _rx) = mpsc::channel();
        let post = core
            .send_room_images(
                &group.id,
                "",
                &[],
                std::slice::from_ref(&asset.id),
                Arc::new(Listener(tx)),
            )
            .unwrap();
        for _ in 0..300 {
            if core
                .participation(&group.id)
                .unwrap()
                .iter()
                .all(|p| p.slot_status != "pending")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(post.tasks.is_empty());
        let maintenance = core.start_fresh_context(&id).unwrap();
        for _ in 0..500 {
            if maintenance.status().status != "running" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            maintenance.status().status,
            "completed",
            "{}",
            maintenance.status().detail
        );
        let (_, review) = server
            .requests
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert_eq!(image_urls(&review, protocol).len(), 1);
        assert!(review.to_string().contains(&asset.id));
        assert!(
            core.workspace().unwrap().personas[0]
                .memory
                .contains("red circle")
        );
        let (tx, rx) = mpsc::channel();
        let next = core
            .send_room_images(
                &persona.conversation_id,
                "Continue from memory.",
                &[],
                &[],
                Arc::new(Listener(tx)),
            )
            .unwrap();
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        let (_, fresh) = server
            .requests
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        assert!(image_urls(&fresh, protocol).is_empty());
        assert!(!fresh.to_string().contains(&asset.id));
        assert!(fresh.to_string().contains("red circle"));
        assert_eq!(
            core.room_messages(&group.id).unwrap()[0].images[0].id,
            asset.id
        );
        assert_eq!(core.context_markers(&group.id).unwrap().len(), 1);
        drop(next);
    }
}
