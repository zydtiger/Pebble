use super::{history_tests::tool, memory_test_support::*, *};
use serde_json::json;
#[test]
fn automatic_worker_runs_after_publication_accepts_input_and_precedes_next_activation() {
    for api in ["chat_completions", "responses"] {
        let (root, core, id) = isolated();
        let final_text = "accepted ".repeat(1200);
        let reply = if api == "responses" {
            super::memory_responses_support::success(&final_text)
        } else {
            success(&final_text)
        };
        let memory = core.workspace().unwrap().personas.remove(0).memory;
        let mut maintenance = stream(tool(
            api,
            "auto-memory",
            "update_memory",
            json!({"content":memory}),
        ));
        maintenance.delay_headers = true;
        let next = if api == "responses" {
            super::memory_responses_support::success("queued reply")
        } else {
            success("queued reply")
        };
        let server = Server::memory(vec![stream(reply), maintenance, stream(next)]);
        configure(root.path(), &server.url, "mock", api, "");
        let p = core.workspace().unwrap().personas.remove(0);
        let prompt = "seen source ".repeat(1200);
        let dummy = core.database.enqueue_turn(&p, &prompt).unwrap();
        let mut r = core.database.preview_turn(&p, &dummy).unwrap();
        let answer =
            json!({"reply":final_text,"state":{"affinity_user":20,"mood":"calm"}}).to_string();
        let out = if api == "responses" {
            super::memory_responses_support::raw_items(&answer)
        } else {
            vec![
                json!({"role":"assistant","reasoning_content":"private-test-reasoning","content":answer}),
            ]
        };
        crate::memory::append(&mut r, &out).unwrap();
        let estimate = crate::images::estimate(
            &crate::live::request_body(p.chat.as_ref().unwrap(), &r, 4096)
                .unwrap()
                .1,
        )
        .unwrap();
        core.database
            .finish_pending(&dummy, MessageStatus::Cancelled, None)
            .unwrap();
        let context_len = (estimate * 100 / 85 + 4096) * 3 / 2;
        let config = std::fs::read_to_string(root.path().join("config.toml"))
            .unwrap()
            .replace("context_len=65536", &format!("context_len={context_len}"));
        std::fs::write(root.path().join("config.toml"), config).unwrap();
        let (first, rx) = start(&core, &id, &prompt);
        let event = terminal(&rx).pop().unwrap();
        assert_eq!(event.kind, ChatEventKind::Completed, "{}", event.text);
        server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let review = server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .1;
        assert_eq!(
            core.maintenance_status(&id).unwrap().unwrap().status,
            "running"
        );
        // Releasing the completed bubble cannot cancel automatic maintenance.
        drop(first);
        let (queued, rx) = start(&core, &id, "queued after automatic review began");
        let history = core.messages(&id).unwrap();
        assert!(
            history
                .iter()
                .any(|m| m.content == "queued after automatic review began")
        );
        assert_eq!(history.last().unwrap().status, MessageStatus::Queued);
        let event = terminal(&rx).pop().unwrap();
        assert_eq!(event.kind, ChatEventKind::Completed, "{}", event.text);
        let next = server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .1;
        let field = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        assert!(
            !review[field]
                .to_string()
                .contains("queued after automatic review began")
        );
        assert!(
            next[field]
                .to_string()
                .contains("queued after automatic review began")
        );
        assert!(!next[field].to_string().contains(&final_text));
        assert_eq!(
            core.maintenance_status(&id).unwrap().unwrap().status,
            "completed"
        );
        assert_eq!(core.context_markers(&p.conversation_id).unwrap().len(), 1);
        drop(queued);
    }
}

#[test]
fn retry_after_budget_change_rollover_references_carried_trigger_and_image_once() {
    for api in ["chat_completions", "responses"] {
        let (root, core, id) = isolated();
        let accepted = |text: &str| {
            if api == "responses" {
                super::memory_responses_support::success(text)
            } else {
                success(text)
            }
        };
        let memory = core.workspace().unwrap().personas.remove(0).memory;
        let server = Server::memory(vec![
            stream(accepted("A accepted")),
            Plan {
                status: 503,
                body: "synthetic failure".into(),
                delay_headers: false,
                delay_tail: false,
            },
            stream(tool(
                api,
                "retry-review",
                "update_memory",
                json!({"content":memory}),
            )),
            stream(accepted("B retried")),
        ]);
        configure(root.path(), &server.url, "mock", api, "");
        let config = std::fs::read_to_string(root.path().join("config.toml"))
            .unwrap()
            .replace("context_len=65536", "context_len=262144");
        std::fs::write(root.path().join("config.toml"), config).unwrap();
        let (first, rx) = start(&core, &id, &"old accumulated context ".repeat(1200));
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
        server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        drop(first);
        let p = core.workspace().unwrap().personas.remove(0);
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(32, 16, image::Rgb([255, 0, 0]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let asset = core.import_image_data(bytes.get_ref()).unwrap();
        // Activate B with real immutable image references; only its model reply fails.
        let (tx, rx) = std::sync::mpsc::channel();
        struct Listener(std::sync::mpsc::Sender<ChatEvent>);
        impl ChatListener for Listener {
            fn on_event(&self, event: ChatEvent) -> Result<(), String> {
                self.0.send(event).map_err(|e| e.to_string())
            }
        }
        let submission = core
            .send_room_images(
                &p.conversation_id,
                "unique retry photo trigger",
                &[],
                std::slice::from_ref(&asset.id),
                std::sync::Arc::new(Listener(tx)),
            )
            .unwrap();
        let failed = &submission.tasks[0];
        let failed_reply = failed.messages.last().unwrap().id.clone();
        let trigger = submission.message.id.clone();
        assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Failed);
        server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let old = core.model_request(&failed.id).unwrap();
        drop(submission);
        let full = crate::images::estimate(
            &crate::live::request_body(p.chat.as_ref().unwrap(), &old, 4096)
                .unwrap()
                .1,
        )
        .unwrap();
        let mut settings = core.settings(None).unwrap();
        settings.chat.context_len = Some((full * 100 / 95 + 4096) * 3 / 2);
        let update = crate::SettingsUpdate {
            chat: settings.chat,
            credential: crate::CredentialChange::Keep,
            persona: None,
        };
        for attempt in 0..100 {
            match core.save_settings(None, &update) {
                Ok(_) => break,
                Err(crate::Error::Busy) if attempt < 99 => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("{e}"),
            }
        }
        let (retried, rx) = retry(&core, &id, &failed_reply);
        let event = terminal(&rx).pop().unwrap();
        assert_eq!(event.kind, ChatEventKind::Completed, "{}", event.text);
        server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let body = server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .1;
        let request = core.model_request(&retried.id).unwrap();
        assert_ne!(request.session_id, old.session_id);
        assert_eq!(request.history_cutoff, old.history_cutoff);
        let native: serde_json::Value =
            serde_json::from_slice(&request.items.last().unwrap().payload).unwrap();
        let envelope: serde_json::Value = serde_json::from_str(
            native["content"]
                .as_str()
                .or(native["content"][0]["text"].as_str())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(envelope["reply_request"]["trigger_message_id"], trigger);
        assert_eq!(
            envelope["reply_request"]["conversation_id"],
            p.conversation_id
        );
        let carry: serde_json::Value =
            serde_json::from_slice(&request.recent_history.as_ref().unwrap().payload).unwrap();
        assert!(carry.to_string().contains(&trigger));
        assert!(request.items.iter().all(|item| {
            !String::from_utf8_lossy(&item.payload).contains("unique retry photo trigger")
        }));
        let field = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        let input = body[field].to_string();
        assert_eq!(input.matches("unique retry photo trigger").count(), 1);
        assert_eq!(input.matches("data:image/png;base64,").count(), 1);
        assert!(input.contains(&trigger));
        assert!(input.contains("trigger_message_id"));
        assert_eq!(
            core.maintenance_status(&id).unwrap().unwrap().status,
            "completed"
        );
    }
}
