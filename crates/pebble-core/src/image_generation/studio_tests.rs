use super::{studio::*, tests::*, *};
use crate::{Core, chat::Control};
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
                                    serde_json::from_slice(&data[index + 4..index + 4 + size])
                                        .unwrap()
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
                    "GET /api/profiles"=>json!({"profiles":[{"profile_id":"qwen","dimension_multiple":32,"min_steps":1,"max_steps":100,"guidance_fixed":1.0}]}).to_string().into_bytes(),
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
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
