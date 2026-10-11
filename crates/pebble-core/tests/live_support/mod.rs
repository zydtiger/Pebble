// Shared HTTP fixtures for deterministic and opt-in live integration tests.
#![allow(dead_code)]
use pebble_core::{ChatEvent, ChatEventKind, ChatListener, Core, Error};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

struct Listener(mpsc::Sender<ChatEvent>);
impl ChatListener for Listener {
    fn on_event(&self, event: ChatEvent) -> Result<(), String> {
        self.0.send(event).map_err(|e| e.to_string())
    }
}
pub(super) struct Plan {
    pub status: u16,
    pub body: String,
    pub delay_headers: bool,
    pub delay_tail: bool,
}
pub(super) fn stream(body: String) -> Plan {
    Plan {
        status: 200,
        body,
        delay_headers: false,
        delay_tail: false,
    }
}
pub(super) fn event(fields: Value, reason: Value) -> String {
    format!(
        "data: {}\r\n\r\n",
        json!({"choices":[{"index":0,"delta":fields,"finish_reason":reason}]})
    )
}
pub(super) fn structured(text: &str) -> String {
    json!({"reply":text,"state":{"affinity_user":20,"mood":"calm"}}).to_string()
}
pub(super) fn success(text: &str) -> String {
    candidate(&structured(text))
}
pub(super) fn candidate(text: &str) -> String {
    format!(
        "{}{}{}data: [DONE]\r\n\r\n",
        event(
            json!({"role":"assistant","reasoning_content":"private-test-reasoning"}),
            Value::Null
        ),
        event(json!({"content":text}), Value::Null),
        event(json!({}), json!("stop"))
    )
}
pub(super) struct Server {
    pub url: String,
    pub requests: mpsc::Receiver<(String, Value)>,
    worker: Option<std::thread::JoinHandle<()>>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}
impl Server {
    pub fn new(plans: Vec<Plan>) -> Self {
        Self::with_maintenance(plans, true)
    }
    pub fn memory(plans: Vec<Plan>) -> Self {
        Self::with_maintenance(plans, false)
    }
    fn with_maintenance(plans: Vec<Plan>, automatic: bool) -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", socket.local_addr().unwrap());
        let (tx, requests) = mpsc::channel();
        socket.set_nonblocking(true).unwrap();
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = shutdown.clone();
        let worker = std::thread::spawn(move || {
            let mut plans = plans.into_iter();
            let mut next = plans.next();
            loop {
                if stopped.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let (mut connection, _) = match socket.accept() {
                    Ok(c) => c,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                connection.set_nonblocking(false).unwrap();
                connection
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 1024];
                let (headers, body) = loop {
                    let n = connection.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(index) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                        let headers = String::from_utf8(bytes[..index].to_vec()).unwrap();
                        let size: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(str::to_owned)
                            })
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= index + 4 + size {
                            break (
                                headers,
                                serde_json::from_slice::<Value>(
                                    &bytes[index + 4..index + 4 + size],
                                )
                                .unwrap(),
                            );
                        }
                    }
                };
                let plan = if automatic && let Some(output) = maintenance_reply(&body) {
                    stream(output)
                } else {
                    let plan = next.take().unwrap();
                    next = plans.next();
                    tx.send((headers, body)).unwrap();
                    plan
                };
                if plan.delay_headers {
                    std::thread::sleep(Duration::from_millis(700));
                }
                let headers = format!(
                    "HTTP/1.1 {} Test\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    plan.status,
                    plan.body.len()
                );
                if connection.write_all(headers.as_bytes()).is_err() {
                    continue;
                }
                // One byte per write deliberately fragments Unicode and SSE boundaries.
                for (index, byte) in plan.body.bytes().enumerate() {
                    if plan.delay_tail
                        && index
                            == plan
                                .body
                                .find("data: [DONE]")
                                .or_else(|| plan.body.find("event: response.completed"))
                                .unwrap_or(plan.body.len() / 2)
                    {
                        std::thread::sleep(Duration::from_millis(700));
                    }
                    if connection.write_all(&[byte]).is_err() {
                        break;
                    }
                }
            }
        });
        Self {
            url,
            requests,
            worker: Some(worker),
            shutdown,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}
// Older behavioral fixtures review unchanged memory without consuming a chat plan.
pub(super) fn maintenance_reply(body: &Value) -> Option<String> {
    let inputs = body["messages"].as_array().or(body["input"].as_array())?;
    let last = inputs.last()?;
    let text = last["content"]
        .as_str()
        .or(last["content"][0]["text"].as_str())?;
    let maintenance = serde_json::from_str::<Value>(text).ok()?;
    maintenance.get("maintenance")?;
    let args = json!({"content":maintenance["current_memory"]}).to_string();
    Some(if body.get("input").is_some() {
        format!(
            "data: {}\n\n",
            json!({"type":"response.completed","response":{"status":"completed","output":[{"id":"maintenance_item","type":"function_call","status":"completed","call_id":"maintenance_call","name":"update_memory","arguments":args}]}})
        )
    } else {
        format!(
            "{}{}data: [DONE]\n\n",
            event(
                json!({"tool_calls":[{"index":0,"id":"maintenance_call","type":"function","function":{"name":"update_memory","arguments":args}}]}),
                Value::Null
            ),
            event(json!({}), json!("tool_calls"))
        )
    })
}
pub(super) fn configure(root: &Path, url: &str, model: &str, api: &str, key: &str) {
    std::fs::write(
        root.join("config.toml"),
        format!(
            "[defaults.chat]\ncontext_len=65536\nmax_output_tokens=4096\nmodel='{model}'\nbase_url='{url}'\napi='{api}'\napi_key='{key}'\n"
        ),
    )
    .unwrap();
}
pub(super) fn isolated() -> (tempfile::TempDir, Core, String) {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    std::fs::write(
        root.path().join("config.toml"),
        "# Classifier disabled in synthetic manual fixtures.\n",
    )
    .unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    std::fs::write(
        Path::new(&persona.directory).join("SOUL.md"),
        "You are a synthetic integration-test character. Answer concisely.",
    )
    .unwrap();
    std::fs::write(
        Path::new(&persona.directory).join("MEMORY.md"),
        "Synthetic test memory only.",
    )
    .unwrap();
    (root, core, persona.id)
}
pub(super) fn start(
    core: &Core,
    id: &str,
    text: &str,
) -> (Arc<pebble_core::ChatTask>, mpsc::Receiver<ChatEvent>) {
    let (tx, rx) = mpsc::channel();
    (
        core.send_message(id, text, Arc::new(Listener(tx))).unwrap(),
        rx,
    )
}
pub(super) fn terminal(rx: &mpsc::Receiver<ChatEvent>) -> Vec<ChatEvent> {
    let mut events = Vec::new();
    loop {
        let event = rx.recv_timeout(Duration::from_secs(180)).unwrap();
        let done = matches!(
            event.kind,
            ChatEventKind::Completed | ChatEventKind::Failed | ChatEventKind::Cancelled
        );
        events.push(event);
        if done {
            return events;
        }
    }
}
pub(super) fn reopen(root: &Path) -> Core {
    for _ in 0..100 {
        match Core::open(root) {
            Ok(core) => return core,
            Err(Error::AlreadyOpen) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => panic!("{e}"),
        }
    }
    panic!("worker still owns root")
}

pub(super) fn retry(
    core: &Core,
    persona: &str,
    reply: &str,
) -> (Arc<pebble_core::ChatTask>, mpsc::Receiver<ChatEvent>) {
    let (tx, rx) = mpsc::channel();
    let task = core
        .retry_message(persona, reply, Arc::new(Listener(tx)))
        .unwrap();
    (task, rx)
}

/// Downgrade fixtures predating per-item encoding must contain raw native JSON.
pub(super) fn raw_history(root: &Path) {
    let db = rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap();
    let rows: Vec<(String, Vec<u8>)> = db
        .prepare("SELECT id,payload FROM model_items WHERE codec='zstd'")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (id, bytes) in rows {
        let raw = zstd::stream::decode_all(std::io::Cursor::new(bytes)).unwrap();
        db.execute(
            "UPDATE model_items SET codec='json',payload=?2 WHERE id=?1",
            rusqlite::params![id, raw],
        )
        .unwrap();
    }
}
