use super::*;
use crate::{Core, MessageStatus, chat::Control};
use image::{ImageFormat, RgbImage};
use std::{io::Cursor, sync::Arc};
pub(super) fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    RgbImage::new(256, 256)
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}
pub(super) fn prompt() -> serde_json::Value {
    serde_json::json!({
        "subject":"One pebble beside a cup labeled \"茶\"",
        "style":"watercolor",
        "composition":"centered on the page",
        "lighting":"soft morning light",
        "background":"white paper",
        "details":"a green leaf\nnext to the cup",
        "constraints":["no border", "preserve the label"]
    })
}
pub(super) fn prepare(
    core: &Core,
    backend: &str,
    settings: &ImageGenerationSettings,
    executable: &str,
) -> String {
    let persona = core.workspace().unwrap().personas.remove(0);
    let queued = core
        .database
        .enqueue_turn(
            &persona,
            "Generate one synthetic watercolor pebble; no text.",
        )
        .unwrap();
    let turn = core.database.activate_turn(&persona, &queued).unwrap();
    let request = core.database.request(&turn.call_id).unwrap();
    let native = serde_json::json!({
        "type":"function_call", "call_id":"synthetic-image", "name":"generate_image",
        "arguments":serde_json::json!({"prompt":prompt(),"backend":backend,"size":{"width":256,"height":256}}).to_string()
    });
    let call = match tool::calls(&[native]).unwrap() {
        tool::Parsed::Valid(call) => call,
        _ => panic!("Expected a valid structured image prompt"),
    };
    let (_, id) = core
        .database
        .prepare_image_generation(&request, &call, &[], settings, executable)
        .unwrap();
    core.database
        .finish_turn(
            &turn,
            "",
            MessageStatus::Failed,
            Some("Synthetic probe skips chat acknowledgement."),
            None,
        )
        .unwrap();
    id.unwrap()
}
#[test]
#[ignore = "Opt-in real backend request; configure PEBBLE_IMAGE_PROBE_BACKEND and backend-specific environment"]
fn live_image_generation_probe() {
    let backend =
        std::env::var("PEBBLE_IMAGE_PROBE_BACKEND").expect("Explicit backend is required");
    let settings = if backend == "studio" {
        ImageGenerationSettings {
            studio: Some(StudioSettings {
                base_url: std::env::var("PEBBLE_IMAGE_PROBE_STUDIO_URL").unwrap(),
                model_id: std::env::var("PEBBLE_IMAGE_PROBE_MODEL_ID").unwrap(),
                gpu_id: std::env::var("PEBBLE_IMAGE_PROBE_GPU_ID").unwrap(),
                steps: None,
                guidance: None,
            }),
            chatgpt: None,
        }
    } else {
        assert_eq!(backend, "chatgpt");
        ImageGenerationSettings {
            studio: None,
            chatgpt: Some(ChatGptImageSettings {
                model: Some(
                    std::env::var("PEBBLE_IMAGE_PROBE_CONTROLLER")
                        .expect("Explicit controller model required"),
                ),
                effort: Some("medium".into()),
            }),
        }
    };
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let executable =
        std::env::var("PEBBLE_IMAGE_PROBE_EXECUTABLE").unwrap_or_else(|_| "codex".into());
    let id = prepare(&core, &backend, &settings, &executable);
    let control = Arc::new(Control::default());
    let result = if backend == "studio" {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(super::studio::execute(
                &core.image_generation,
                &id,
                false,
                &control,
            ))
    } else {
        super::chatgpt::execute(&core.image_generation, &id, false, &control)
    };
    if let Err(error) = &result {
        core.database
            .update_image_generation(&id, |o| {
                o.status = if o.dispatch_started {
                    "uncertain"
                } else {
                    "failed"
                }
                .into();
                o.activity.clear();
                o.detail = error.to_string();
                Ok(())
            })
            .unwrap();
    }
    let op = core.database.image_generation(&id).unwrap();
    let metadata = serde_json::json!({"backend":backend,"operation_id":id,"remote_id":op.remote_id,"turn_id":op.turn_id,"item_id":op.item_id,"artifact_id":op.artifact_id,"status":op.status,"requested_size":op.arguments.size,"effective_parameters":op.effective_parameters,"width":op.actual_width,"height":op.actual_height,"detail":op.detail});
    if let Ok(output) = std::env::var("PEBBLE_IMAGE_PROBE_OUTPUT_DIR") {
        let output = std::path::Path::new(&output);
        assert!(output.is_absolute());
        std::fs::create_dir_all(output).unwrap();
        use std::io::Write;
        let mut evidence = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(format!("{backend}-{id}.json")))
            .unwrap();
        evidence
            .write_all(&serde_json::to_vec_pretty(&metadata).unwrap())
            .unwrap();
        if let Some(asset_id) = &op.asset_id {
            let source = root
                .path()
                .join("data/assets")
                .join(asset_id)
                .join("source.png");
            let destination = output.join(format!("{backend}-{id}.png"));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .unwrap();
            file.write_all(&std::fs::read(source).unwrap()).unwrap();
        }
    }
    println!("{metadata}");
    assert!(
        result.is_ok(),
        "Generation failed; identity and outcome were preserved."
    );
    assert_eq!(
        op.status, "completed",
        "Backend result was not successful; evidence was preserved."
    );
    assert!(op.asset_id.is_some());
}

use crate::chat::{
    memory_responses_support as responses_support, memory_test_support as live_support,
};
fn native_image(api: &str) -> String {
    let args=serde_json::json!({"prompt":prompt(),"backend":"chatgpt","size":{"width":256,"height":256}}).to_string();
    if api == "responses" {
        format!(
            "data: {}\n\n",
            serde_json::json!({"type":"response.completed","response":{"status":"completed","output":[{"id":"native-image","type":"function_call","status":"completed","call_id":"image-call","name":"generate_image","arguments":args}]}})
        )
    } else {
        format!(
            "{}{}data: [DONE]\n\n",
            live_support::event(
                serde_json::json!({"tool_calls":[{"index":0,"id":"image-call","type":"function","function":{"name":"generate_image","arguments":args}}]}),
                serde_json::Value::Null
            ),
            live_support::event(serde_json::json!({}), serde_json::json!("tool_calls"))
        )
    }
}
fn final_reply(api: &str, text: &str) -> String {
    if api == "responses" {
        responses_support::success(text)
    } else {
        live_support::success(text)
    }
}
#[test]
fn image_acknowledgment_retry_releases_queue_without_duplicate_generation() {
    use live_support::*;
    use std::time::{Duration, Instant};
    for api in ["chat_completions", "responses"] {
        let server = Server::memory(vec![
            stream(native_image(api)),
            Plan {
                status: 500,
                body: "synthetic acknowledgement failure".into(),
                delay_headers: false,
                delay_tail: false,
            },
            stream(final_reply(
                api,
                "Image accepted; we can continue chatting.",
            )),
            stream(final_reply(api, "Here is the actual generated image.")),
        ]);
        let (root, core, id) = isolated();
        configure(root.path(), &server.url, "mock", api, "");
        let executable = super::chatgpt_tests::fake(root.path(), false, true);
        core.save_codex_settings(&crate::CodexSettings {
            enabled: false,
            executable,
            writable: false,
            model: Some("delegation-model-must-not-leak".into()),
            effort: Some("high".into()),
        })
        .unwrap();
        core.save_image_generation_settings(&ImageGenerationSettings {
            studio: None,
            chatgpt: Some(ChatGptImageSettings::default()),
        })
        .unwrap();
        let (handle, events) = start(&core, &id, "Generate one image.");
        assert_eq!(
            terminal(&events).last().unwrap().kind,
            crate::ChatEventKind::Failed
        );
        let failed = core.messages(&id).unwrap().pop().unwrap();
        let (_retry, events) = retry(&core, &id, &failed.id);
        assert_eq!(
            terminal(&events).last().unwrap().kind,
            crate::ChatEventKind::Completed
        );
        // The same persona spoke again while the generator still awaited its fixture gate.
        assert!(core.image_generations().unwrap()[0].active());
        std::fs::write(root.path().join("release-image"), "").unwrap();
        let until = Instant::now() + Duration::from_secs(8);
        loop {
            let ops = core.image_generations().unwrap();
            if ops[0].publication_status == "completed" {
                break;
            }
            assert!(Instant::now() < until, "Image report timed out: {ops:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let messages = core.messages(&id).unwrap();
        assert_eq!(messages.last().unwrap().images.len(), 1);
        let methods = std::fs::read_to_string(root.path().join("image-methods.jsonl")).unwrap();
        assert_eq!(
            methods
                .lines()
                .filter(
                    |s| serde_json::from_str::<serde_json::Value>(s).unwrap()["method"]
                        == "turn/start"
                )
                .count(),
            1
        );
        let start: serde_json::Value = methods
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .find(|v: &serde_json::Value| v["method"] == "thread/start")
            .unwrap();
        assert!(start["params"].get("model").is_none());
        let bodies: Vec<_> = (0..4)
            .map(|_| {
                server
                    .requests
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap()
                    .1
            })
            .collect();
        assert_eq!(bodies[2]["tool_choice"], "none");
        assert!(bodies[3].to_string().contains("data:image/png;base64,"));
        assert_eq!(bodies[3]["tool_choice"], "none");
        drop(handle);
    }
}
