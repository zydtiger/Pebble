use super::{chatgpt::*, tests::*, *};
use crate::{Core, chat::Control};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, sync::Arc};
pub(super) fn fake(root: &std::path::Path, lost: bool, gate: bool) -> String {
    let script = root.join("image-app-server");
    std::fs::write(&script, include_str!("fake_app_server.py")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        root.join("image-options.json"),
        json!({"lost":lost,"gate":gate,"image":STANDARD.encode(png())}).to_string(),
    )
    .unwrap();
    script.display().to_string()
}
fn configured() -> ImageGenerationSettings {
    ImageGenerationSettings {
        studio: None,
        chatgpt: Some(ChatGptImageSettings {
            model: Some("image-controller".into()),
            effort: Some("medium".into()),
        }),
    }
}
#[test]
fn typed_images_are_saved_and_recovery_uses_thread_read_without_turn_start() {
    for lost in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let executable = fake(root.path(), lost, false);
        let id = prepare(&core, "chatgpt", &configured(), &executable);
        let control = Arc::new(Control::default());
        let result = execute(&core.image_generation, &id, false, &control);
        assert_eq!(result.is_err(), lost);
        if lost {
            let op = core.database.image_generation(&id).unwrap();
            assert!(op.dispatch_started);
            assert!(op.turn_id.is_none());
            assert_eq!(op.remote_id.as_deref(), Some("image-thread"));
        }
        execute(&core.image_generation, &id, true, &control).unwrap();
        let op = core.database.image_generation(&id).unwrap();
        assert_eq!(op.status, "completed");
        assert_eq!(op.item_id.as_deref(), Some("image-item"));
        assert_eq!(op.actual_width, Some(256));
        assert!(op.asset_id.is_some());
        let methods = std::fs::read_to_string(root.path().join("image-methods.jsonl")).unwrap();
        let methods: Vec<serde_json::Value> = methods
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(
            methods
                .iter()
                .filter(|m| m["method"] == "turn/start")
                .count(),
            1
        );
        assert_eq!(
            methods
                .iter()
                .filter(|m| m["method"] == "thread/read")
                .count(),
            1
        );
        let start = methods
            .iter()
            .find(|m| m["method"] == "thread/start")
            .unwrap();
        assert_eq!(start["params"]["model"], "image-controller");
        assert_eq!(start["params"]["approvalPolicy"], "never");
        assert_eq!(start["params"]["sandbox"], "workspace-write");
    }
}
#[test]
fn dimension_mismatch_preserves_chatgpt_success_but_studio_cannot_publish_success() {
    for backend in ["chatgpt", "studio"] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let mut settings = configured();
        if backend == "studio" {
            settings.studio = Some(StudioSettings {
                base_url: "https://api.example.com".into(),
                model_id: "model".into(),
                gpu_id: "gpu".into(),
                steps: None,
                guidance: None,
            });
        }
        let id = prepare(&core, backend, &settings, "unused");
        core.database
            .update_image_generation(&id, |o| {
                o.arguments.size.width = 512;
                o.status = "running".into();
                Ok(())
            })
            .unwrap();
        core.image_generation.import(&id, &png()).unwrap();
        let op = core.database.image_generation(&id).unwrap();
        assert!(op.asset_id.is_some());
        assert!(
            op.detail
                .contains("Requested 512 × 256; generated 256 × 256")
        );
        assert_eq!(
            op.status,
            if backend == "studio" {
                "failed"
            } else {
                "running"
            }
        );
    }
}

#[test]
fn managed_chatgpt_stop_interrupts_child_and_invalidates_publication() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let executable = fake(root.path(), false, false);
    let options = root.path().join("image-options.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&options).unwrap()).unwrap();
    value["cancel"] = json!(true);
    std::fs::write(options, value.to_string()).unwrap();
    let id = prepare(&core, "chatgpt", &configured(), &executable);
    core.image_generation.dispatch(&id).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while core
        .database
        .image_generation(&id)
        .unwrap()
        .turn_id
        .is_none()
    {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    core.stop_room(&core.database.image_generation(&id).unwrap().conversation_id)
        .unwrap();
    while core.database.image_generation(&id).unwrap().active() {
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let op = core.database.image_generation(&id).unwrap();
    assert_eq!(op.status, "cancelled");
    assert_eq!(op.publication_status, "cancelled");
    assert!(op.asset_id.is_none());
    assert!(core.database.enqueue_image_report(&id).unwrap().is_none());
}

#[test]
fn image_generation_availability_checks_login_capability_and_caches_only_success() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let executable = fake(root.path(), false, false);
    core.save_codex_settings(&crate::CodexSettings {
        executable,
        ..Default::default()
    })
    .unwrap();
    let settings = configured();
    core.save_image_generation_settings(&settings).unwrap();
    assert_eq!(
        core.image_generation.available_backends().unwrap(),
        ["chatgpt"]
    );
    assert_eq!(
        core.image_generation.available_backends().unwrap(),
        ["chatgpt"]
    );
    let methods = std::fs::read_to_string(root.path().join("image-methods.jsonl")).unwrap();
    assert_eq!(
        methods
            .lines()
            .filter(|l| l.contains("account/read"))
            .count(),
        1
    );
    assert!(!methods.contains("thread/start"));
    let options = root.path().join("image-options.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&options).unwrap()).unwrap();
    value["capable"] = json!(false);
    std::fs::write(&options, value.to_string()).unwrap();
    core.save_image_generation_settings(&settings).unwrap();
    assert!(
        core.image_generation
            .available_backends()
            .unwrap()
            .is_empty()
    );
    value["capable"] = json!(true);
    value["account_type"] = json!("apiKey");
    std::fs::write(&options, value.to_string()).unwrap();
    assert!(
        core.image_generation
            .available_backends()
            .unwrap()
            .is_empty()
    );
    value["account_type"] = json!("chatgpt");
    std::fs::write(&options, value.to_string()).unwrap();
    assert_eq!(
        core.image_generation.available_backends().unwrap(),
        ["chatgpt"]
    );
    core.save_image_generation_settings(&ImageGenerationSettings {
        studio: Some(StudioSettings {
            base_url: "https://api.example.com".into(),
            model_id: "model".into(),
            gpu_id: "gpu".into(),
            steps: None,
            guidance: None,
        }),
        chatgpt: None,
    })
    .unwrap();
    assert_eq!(
        core.image_generation.available_backends().unwrap(),
        ["studio"]
    );
}

#[test]
fn typed_chatgpt_failure_is_durable_terminal_and_never_retried() {
    for failure in [
        serde_json::Value::Null,
        json!({"type":"usageLimitExceeded","limitId":"image-generation","resetsAt":123456}),
    ] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let executable = fake(root.path(), false, false);
        let path = root.path().join("image-options.json");
        let mut options: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        options["failed"] = json!(true);
        options["failure"] = failure.clone();
        std::fs::write(path, options.to_string()).unwrap();
        let id = prepare(&core, "chatgpt", &configured(), &executable);
        execute(
            &core.image_generation,
            &id,
            false,
            &Arc::new(Control::default()),
        )
        .unwrap();
        let op = core.database.image_generation(&id).unwrap();
        assert_eq!(op.status, "failed");
        assert!(!op.active());
        assert!(op.asset_id.is_none());
        assert_eq!(op.effective_parameters["failure"], failure);
        assert!(op.detail.contains(if failure.is_null() {
            "without a backend failure reason"
        } else {
            "usage limit exceeded"
        }));
        let methods = std::fs::read_to_string(root.path().join("image-methods.jsonl")).unwrap();
        assert_eq!(
            methods.lines().filter(|l| l.contains("turn/start")).count(),
            1
        );
    }
}

#[test]
fn published_uncertain_image_reconciles_after_restart_without_duplicate_group_report() {
    use crate::chat::memory_test_support::{Server, configure, isolated, stream, success};
    use std::time::{Duration, Instant};
    let server = Server::memory(vec![
        stream(success("Here is the saved image.")),
        stream(success("A duplicate report must never be requested.")),
    ]);
    let (root, core, owner_id) = isolated();
    configure(
        root.path(),
        &server.url,
        "synthetic",
        "chat_completions",
        "",
    );
    let other = core.create_persona("Other").unwrap();
    let group = core
        .create_group("Image recovery", &[owner_id.clone(), other.id.clone()])
        .unwrap();
    let workspace = core.workspace().unwrap();
    let owner = workspace
        .personas
        .iter()
        .find(|p| p.id == owner_id)
        .unwrap();
    let (user, _, _) = core
        .database
        .enqueue_room(
            &group.id,
            "Generate one synthetic image.",
            std::slice::from_ref(&owner_id),
            &workspace.personas,
        )
        .unwrap();
    let origin = match core
        .database
        .step_round(&user.id, &workspace.personas)
        .unwrap()
    {
        crate::storage::rounds::Step::Admit(turn, _) => *turn,
        _ => panic!("Image owner was not admitted"),
    };
    let origin = core.database.activate_turn(owner, &origin).unwrap();
    let executable = fake(root.path(), false, false);
    let options_path = root.path().join("image-options.json");
    let mut options: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&options_path).unwrap()).unwrap();
    options["disconnect_after_image"] = json!(true);
    std::fs::write(&options_path, options.to_string()).unwrap();
    let (_, id) = core
        .database
        .prepare_image_generation(
            &core.database.request(&origin.call_id).unwrap(),
            &tool::Call {
                id: "recoverable-image".into(),
                arguments: tool::Arguments {
                    prompt: "One synthetic pebble.".into(),
                    backend: "chatgpt".into(),
                    size: tool::Size {
                        width: 256,
                        height: 256,
                    },
                },
            },
            &[],
            &configured(),
            &executable,
        )
        .unwrap();
    let id = id.unwrap();
    core.database
        .finish_turn(
            &origin,
            "",
            crate::MessageStatus::Failed,
            Some("Synthetic acknowledgement skipped."),
            None,
        )
        .unwrap();
    core.image_generation.dispatch(&id).unwrap();
    let until = Instant::now() + Duration::from_secs(8);
    let published = loop {
        let op = core.database.image_generation(&id).unwrap();
        if op.publication_status == "completed" {
            break op;
        }
        assert!(Instant::now() < until, "Initial report timed out: {op:?}");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(published.status, "uncertain");
    assert!(published.asset_id.is_some());
    assert_eq!(published.reports.len(), 1);
    server
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let before_messages = core.room_messages(&group.id).unwrap();
    let image_message = before_messages
        .iter()
        .find(|m| !m.images.is_empty())
        .unwrap();
    assert_eq!(
        image_message.images[0].id,
        published.asset_id.clone().unwrap()
    );
    let fanout = || -> i64 {
        rusqlite::Connection::open_with_flags(root.path().join("data/app.sqlite"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap().query_row(
            "SELECT COUNT(*) FROM persona_events WHERE persona_id=?1 AND message_id=?2 AND kind='received'",
            rusqlite::params![other.id, image_message.id], |r| r.get(0),
        ).unwrap()
    };
    assert_eq!(fanout(), 1);
    drop(core);
    let until = Instant::now() + Duration::from_secs(8);
    let core = loop {
        match Core::open(root.path()) {
            Ok(core) => break core,
            Err(crate::Error::AlreadyOpen) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(error) => panic!("{error}"),
        }
    };
    loop {
        let op = core.database.image_generation(&id).unwrap();
        if op.status == "completed" {
            // Join the worker so the publication-reset/report step has also finished.
            core.image_generation.shutdown();
            break;
        }
        assert!(
            Instant::now() < until,
            "Read-only recovery timed out: {op:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    core.image_generation.reconcile(&id).unwrap();
    let recovered = core.database.image_generation(&id).unwrap();
    assert_eq!(recovered.publication_status, "completed");
    assert_eq!(recovered.asset_id, published.asset_id);
    assert_eq!(recovered.report_call, published.report_call);
    assert_eq!(recovered.reports.len(), 1);
    assert_eq!(
        core.room_messages(&group.id).unwrap().len(),
        before_messages.len()
    );
    assert_eq!(fanout(), 1);
    assert!(server.requests.try_recv().is_err());
    let methods = std::fs::read_to_string(root.path().join("image-methods.jsonl")).unwrap();
    assert_eq!(
        methods.lines().filter(|l| l.contains("turn/start")).count(),
        1
    );
    assert_eq!(
        methods
            .lines()
            .filter(|l| l.contains("thread/read"))
            .count(),
        1
    );
}
