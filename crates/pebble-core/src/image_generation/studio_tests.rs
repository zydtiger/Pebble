use super::{studio::*, tests::*, *};
use crate::{Core, chat::Control};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
struct Server {
    url: String,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(mode: &str) -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let url = format!("http://{}", socket.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = stop.clone();
        let mode = mode.to_owned();
        let worker = std::thread::spawn(move || {
            let mut cancelled = false;
            while !shutdown.load(Ordering::Relaxed) {
                let (mut stream, _) = match socket.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut data = Vec::new();
                let mut buf = [0; 1024];
                let (route, body) = loop {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    data.extend_from_slice(&buf[..n]);
                    if let Some(index) = data.windows(4).position(|b| b == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&data[..index]);
                        let size: usize = headers
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_owned)
                            })
                            .unwrap_or_else(|| "0".into())
                            .parse()
                            .unwrap();
                        if data.len() >= index + 4 + size {
                            break (
                                headers
                                    .lines()
                                    .next()
                                    .unwrap()
                                    .split_whitespace()
                                    .take(2)
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                if size > 0 {
                                    if headers.contains("multipart/form-data") {
                                        json!({"headers":headers,"multipart":STANDARD.encode(&data[index+4..index+4+size])})
                                    } else {
                                        serde_json::from_slice(&data[index + 4..index + 4 + size])
                                            .unwrap()
                                    }
                                } else {
                                    Value::Null
                                },
                            );
                        }
                    }
                };
                captured.lock().unwrap().push((route.clone(), body));
                let response=match route.as_str() {
                    "GET /api/models"=>json!({"registrations":[{"id":"model","status":"ready","profile":"qwen"}]}).to_string().into_bytes(),
                    "GET /api/profiles"=>json!({"profiles":[{"profile_id":"qwen","dimension_multiple":if mode=="large-multiple"{2048}else{32},"min_steps":1,"max_steps":100,"guidance_fixed":1.0,"generation_modes":if mode=="no-edit" {vec!["text-to-image"]} else {vec!["text-to-image","image-edit"]},"max_input_images":1,"max_input_bytes":if mode=="small-input"{1}else{20971520},"max_input_pixels":16777216}]}).to_string().into_bytes(),
                    "POST /api/inputs" if mode=="lost-input"=>{continue;},
                    "POST /api/inputs"=>json!({"input_id":"saved-input","created_at":"now","width":256,"height":256,"size_bytes":png().len(),"url":"/api/inputs/saved-input"}).to_string().into_bytes(),
                    "POST /api/generations" if mode=="lost"=>{continue;},
                    "POST /api/generations"=>json!({"run_id":"saved-run","steps":40,"guidance":1.0,"profile":"qwen","commit_sha":"accepted-revision","gpu":{"uuid":"gpu"}}).to_string().into_bytes(),
                    "POST /api/generations/saved-run/cancel"=>{cancelled=true;json!({"status":"cancelling"}).to_string().into_bytes()},
                    "GET /api/generations/saved-run" if mode.starts_with("cancel")=>json!({"run_id":"saved-run","status":if cancelled && mode=="cancel" {"cancelled"} else {"running"},"images":[]}).to_string().into_bytes(),
                    "GET /api/generations/saved-run"=>json!({"run_id":"saved-run","status":"completed","commit_sha":"resolved-revision","images":[{"artifact_id":"image-001","status":"completed"}]}).to_string().into_bytes(),
                    "GET /api/generations/saved-run/artifacts/image-001"=>png(),
                    _=>panic!("Unexpected Studio request {route}"),
                };
                write!(
                    stream,
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    if route == "POST /api/inputs" {
                        "201 Created"
                    } else {
                        "200 OK"
                    },
                    response.len()
                )
                .unwrap();
                let _ = stream.write_all(&response);
            }
        });
        Self {
            url,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn configured(url: &str) -> ImageGenerationSettings {
    ImageGenerationSettings {
        studio: Some(StudioSettings {
            base_url: url.into(),
            model_id: "model".into(),
            gpu_id: "gpu".into(),
            steps: None,
            guidance: None,
        }),
        chatgpt: None,
    }
}
#[test]
fn studio_submits_once_omits_overrides_imports_and_recovers_by_get_only() {
    let server = Server::new("complete");
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let id = prepare(&core, "studio", &configured(&server.url), "unused");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let control = Arc::new(Control::default());
    rt.block_on(execute(&core.image_generation, &id, false, &control))
        .unwrap();
    let op = core.database.image_generation(&id).unwrap();
    assert_eq!(op.status, "completed");
    assert_eq!(op.actual_width, Some(256));
    assert_eq!(op.effective_parameters["steps"], 40);
    assert_eq!(op.effective_parameters["guidance"], 1.0);
    assert_eq!(op.effective_parameters["commit_sha"], "resolved-revision");
    let asset = op.asset_id;
    let requests = server.requests.lock().unwrap();
    let post = requests
        .iter()
        .find(|(r, _)| r == "POST /api/generations")
        .unwrap();
    assert_eq!(post.1["count"], 1);
    let payload = post.1["prompt"].as_str().expect("Studio receives a string");
    assert_eq!(payload, op.arguments.prompt);
    assert_eq!(serde_json::from_str::<Value>(payload).unwrap(), prompt());
    assert!(post.1.get("steps").is_none());
    assert!(post.1.get("guidance").is_none());
    let count = requests.len();
    drop(requests);
    // Recovery does not consult editable settings or submit again.
    rt.block_on(execute(&core.image_generation, &id, true, &control))
        .unwrap();
    assert_eq!(core.database.image_generation(&id).unwrap().asset_id, asset);
    assert_eq!(
        core.database
            .image_generation(&id)
            .unwrap()
            .effective_parameters["steps"],
        40
    );
    let requests = server.requests.lock().unwrap();
    assert!(requests[count..].iter().all(|(r, _)| r.starts_with("GET ")));
    assert_eq!(
        requests
            .iter()
            .filter(|(r, _)| r == "POST /api/generations")
            .count(),
        1
    );
    drop(requests);
    drop(core);
    let reopened = Core::open(root.path()).unwrap();
    let saved = reopened.database.image_generation(&id).unwrap();
    assert_eq!(saved.effective_parameters["steps"], 40);
    assert_eq!(
        saved.effective_parameters["commit_sha"],
        "resolved-revision"
    );
}
#[test]
fn lost_studio_submission_stays_uncertain_without_retry() {
    let server = Server::new("lost");
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let id = prepare(&core, "studio", &configured(&server.url), "unused");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let control = Arc::new(Control::default());
    assert!(
        rt.block_on(execute(&core.image_generation, &id, false, &control))
            .is_err()
    );
    let op = core.database.image_generation(&id).unwrap();
    assert!(op.dispatch_started);
    assert!(op.remote_id.is_none());
    assert!(
        rt.block_on(execute(&core.image_generation, &id, true, &control))
            .is_err()
    );
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(r, _)| r == "POST /api/generations")
            .count(),
        1
    );
}

#[test]
fn studio_character_limit_counts_serialized_json_before_any_http_request() {
    let server = Server::new("complete");
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let id = prepare(&core, "studio", &configured(&server.url), "unused");
    // The unescaped visual description fits; its JSON payload exceeds 8000 characters.
    let prompt = json!({"subject":"\"".repeat(4000)}).to_string();
    assert!(prompt.len() < 16384);
    assert!(prompt.chars().count() > 8000);
    core.database
        .update_image_generation(&id, |o| {
            o.arguments.prompt = prompt;
            Ok(())
        })
        .unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = rt
        .block_on(execute(
            &core.image_generation,
            &id,
            false,
            &Arc::new(Control::default()),
        ))
        .unwrap_err();
    assert!(error.to_string().contains("8000 characters"));
    assert!(
        !core
            .database
            .image_generation(&id)
            .unwrap()
            .dispatch_started
    );
    assert!(server.requests.lock().unwrap().is_empty());
}

#[test]
fn legacy_studio_prompt_recovers_saved_run_without_conversion_or_resubmission() {
    let server = Server::new("complete");
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let id = prepare(&core, "studio", &configured(&server.url), "unused");
    let legacy = "Original watercolor pebble; keep \"茶\".\nNo border.";
    core.database
        .update_image_generation(&id, |o| {
            o.arguments.prompt = legacy.into();
            o.dispatch_started = true;
            o.remote_id = Some("saved-run".into());
            Ok(())
        })
        .unwrap();
    core.database.recover_image_generations().unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(execute(
        &core.image_generation,
        &id,
        true,
        &Arc::new(Control::default()),
    ))
    .unwrap();
    let op = core.database.image_generation(&id).unwrap();
    assert_eq!(op.status, "completed");
    assert_eq!(op.arguments.prompt, legacy);
    assert!(op.asset_id.is_some());
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(r, _)| r.starts_with("GET "))
    );
}

#[test]
fn studio_stop_has_bounded_confirmation_and_never_resumes_shared_queue() {
    for mode in ["cancel", "cancel-unconfirmed"] {
        let server = Server::new(mode);
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let id = prepare(&core, "studio", &configured(&server.url), "unused");
        let control = Arc::new(Control::default());
        let manager = core.image_generation.clone();
        let worker_control = control.clone();
        let worker_id = id.clone();
        let worker = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(execute(&manager, &worker_id, false, &worker_control))
        });
        let until = std::time::Instant::now() + Duration::from_secs(3);
        while core
            .database
            .image_generation(&id)
            .unwrap()
            .remote_id
            .is_none()
        {
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(Duration::from_millis(5));
        }
        core.stop_room(&core.database.image_generation(&id).unwrap().conversation_id)
            .unwrap();
        control.cancel();
        let result = worker.join().unwrap();
        assert_eq!(result.is_ok(), mode == "cancel");
        let op = core.database.image_generation(&id).unwrap();
        assert!(op.invalidated);
        assert_eq!(op.publication_status, "cancelled");
        assert!(op.asset_id.is_none());
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|(r, _)| r.ends_with("/cancel"))
                .count(),
            1
        );
        assert!(!requests.iter().any(|(r, _)| r.contains("queue/resume")));
    }
}

#[test]
fn studio_edit_uploads_original_once_and_freezes_exact_edit_submission() {
    let server = Server::new("complete");
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let (id, source) = prepare_edit(&core, "studio", &configured(&server.url), "unused");
    let original = std::fs::read(&source.source_path).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let control = Arc::new(Control::default());
    rt.block_on(execute(&core.image_generation, &id, false, &control))
        .unwrap();
    let op = core.database.image_generation(&id).unwrap();
    assert_eq!(op.kind, ImageOperationKind::Edit);
    assert_eq!(op.input_id.as_deref(), Some("saved-input"));
    assert_ne!(op.asset_id.as_deref(), Some(source.id.as_str()));
    assert_eq!(std::fs::read(&source.source_path).unwrap(), original);
    let requests = server.requests.lock().unwrap();
    let upload = requests
        .iter()
        .find(|(r, _)| r == "POST /api/inputs")
        .unwrap();
    let raw = STANDARD
        .decode(upload.1["multipart"].as_str().unwrap())
        .unwrap();
    assert!(raw.windows(original.len()).any(|w| w == original));
    assert_eq!(
        (
            op.source.as_ref().unwrap().width,
            op.source.as_ref().unwrap().height
        ),
        (1024, 768)
    );
    assert!(String::from_utf8_lossy(&raw).contains("name=\"file\"; filename=\"source.png\""));
    let submission = &requests
        .iter()
        .find(|(r, _)| r == "POST /api/generations")
        .unwrap()
        .1;
    assert_eq!(submission["mode"], "image-edit");
    assert_eq!(submission["input_image_ids"], json!(["saved-input"]));
    assert_eq!(submission["count"], 1);
    assert_eq!(submission["prompt"], op.arguments.prompt);
    let count = requests.len();
    drop(requests);
    rt.block_on(execute(&core.image_generation, &id, true, &control))
        .unwrap();
    assert!(
        rt.block_on(execute(&core.image_generation, &id, false, &control))
            .is_err()
    );
    let requests = server.requests.lock().unwrap();
    assert!(requests[count..].iter().all(|(r, _)| r.starts_with("GET ")));
    assert_eq!(
        requests
            .iter()
            .filter(|(r, _)| r == "POST /api/inputs")
            .count(),
        1
    );
}
#[test]
fn studio_edit_profile_rejects_before_upload_and_lost_upload_or_submit_never_repeats() {
    for mode in ["no-edit", "small-input", "lost-input", "lost"] {
        let server = Server::new(mode);
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let (id, _) = prepare_edit(&core, "studio", &configured(&server.url), "unused");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let control = Arc::new(Control::default());
        assert!(
            rt.block_on(execute(&core.image_generation, &id, false, &control))
                .is_err()
        );
        core.database.recover_image_generations().unwrap();
        let op = core.database.image_generation(&id).unwrap();
        if mode.starts_with("lost") {
            assert!(op.dispatch_started);
            assert_eq!(op.status, "uncertain");
            assert_eq!(op.input_id.is_some(), mode == "lost");
            assert!(
                rt.block_on(execute(&core.image_generation, &id, false, &control))
                    .is_err()
            );
        } else {
            assert!(!op.dispatch_started);
        }
        assert!(
            rt.block_on(execute(&core.image_generation, &id, true, &control))
                .is_err()
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|(r, _)| r == "POST /api/inputs")
                .count(),
            usize::from(mode.starts_with("lost"))
        );
        assert_eq!(
            requests
                .iter()
                .filter(|(r, _)| r == "POST /api/generations")
                .count(),
            usize::from(mode == "lost")
        );
    }
}

#[test]
fn studio_unsupported_original_size_fails_before_upload_without_resizing() {
    let server = Server::new("large-multiple");
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let (id, _) = prepare_edit(&core, "studio", &configured(&server.url), "unused");
    core.database
        .update_image_generation(&id, |o| {
            o.arguments.size = tool::Size {
                width: 1024,
                height: 768,
            };
            Ok(())
        })
        .unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let error = rt
        .block_on(execute(
            &core.image_generation,
            &id,
            false,
            &Arc::new(Control::default()),
        ))
        .unwrap_err();
    assert!(error.to_string().contains("pixel multiple"));
    let op = core.database.image_generation(&id).unwrap();
    assert_eq!(
        (op.arguments.size.width, op.arguments.size.height),
        (1024, 768)
    );
    assert!(!op.dispatch_started && op.input_id.is_none() && op.remote_id.is_none());
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(r, _)| r.starts_with("GET "))
    );
}
