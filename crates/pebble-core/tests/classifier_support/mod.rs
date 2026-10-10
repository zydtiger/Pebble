//! Synthetic bounded HTTP server shared by classifier integration checks.
#![allow(dead_code)]
use pebble_core::{ChatEvent, ChatEventKind, ChatListener, ChatSubmission, Core};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

pub struct Reply {
    pub body: Value,
    pub delay: Duration,
    pub status: u16,
    pub stream_body: Option<String>,
}
impl Reply {
    pub fn json(body: Value) -> Self {
        Self {
            body,
            delay: Duration::ZERO,
            status: 200,
            stream_body: None,
        }
    }
    pub fn stream(body: String) -> Self {
        let mut reply = Self::json(Value::Null);
        reply.stream_body = Some(body);
        reply
    }
    pub fn delayed(mut self, ms: u64) -> Self {
        self.delay = Duration::from_millis(ms);
        self
    }
}
pub fn answer(api: &str, text: &str) -> Value {
    if api == "responses" {
        json!({"status":"completed","output":[{"id":"synthetic-classifier-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text}]}]})
    } else {
        json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":text}}]})
    }
}
pub struct Server {
    pub url: String,
    pub requests: mpsc::Receiver<(String, Value)>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    pub fn new(plans: Vec<Reply>) -> Self {
        let plans = Mutex::new(std::collections::VecDeque::from(plans));
        Self::handling(move |_| {
            plans.lock().unwrap().pop_front().unwrap_or_else(|| Reply {
                body: json!({"error":"unexpected request"}),
                delay: Duration::ZERO,
                status: 500,
                stream_body: None,
            })
        })
    }
    pub fn handling(handler: impl Fn(&Value) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let (tx, requests) = mpsc::channel();
        let handler = Arc::new(handler);
        let worker = std::thread::spawn(move || {
            let mut children = Vec::new();
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let handler = handler.clone();
                        let tx = tx.clone();
                        children.push(std::thread::spawn(move || {
                            stream.set_nonblocking(false).unwrap();
                            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                            let mut bytes=Vec::new();let mut chunk=[0;4096];
                            let (head,body)=loop {
                                let Ok(n)=stream.read(&mut chunk) else { return; };if n==0 {return;}bytes.extend_from_slice(&chunk[..n]);
                                if let Some(end)=bytes.windows(4).position(|w|w==b"\r\n\r\n") {
                                    let head=String::from_utf8(bytes[..end].to_vec()).unwrap();
                                    let len:usize=head.lines().find_map(|l|l.to_lowercase().strip_prefix("content-length:").map(|v|v.trim().parse().unwrap())).unwrap();
                                    if bytes.len()>=end+4+len {break(head,serde_json::from_slice(&bytes[end+4..end+4+len]).unwrap());}
                                }
                            };
                            let reply=handler(&body);let _=tx.send((head,body));
                            std::thread::sleep(reply.delay);
                            let content_type=if reply.stream_body.is_some() {"text/event-stream"} else {"application/json"};
                            let body=reply.stream_body.unwrap_or_else(||reply.body.to_string());let response=format!("HTTP/1.1 {} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",reply.status,body.len(),body);
                            let _=stream.write_all(response.as_bytes());
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("{error}"),
                }
            }
            for child in children {
                child.join().unwrap();
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
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}
struct Listener(mpsc::Sender<ChatEvent>);
impl ChatListener for Listener {
    fn on_event(&self, event: ChatEvent) -> Result<(), String> {
        self.0.send(event).map_err(|e| e.to_string())
    }
}
pub fn submit(
    core: &Core,
    room: &str,
    text: &str,
    picked: &[String],
) -> (ChatSubmission, mpsc::Receiver<ChatEvent>) {
    let (tx, rx) = mpsc::channel();
    (
        core.send_room_message(room, text, picked, Arc::new(Listener(tx)))
            .unwrap(),
        rx,
    )
}
pub fn finish(submission: &ChatSubmission, rx: &mpsc::Receiver<ChatEvent>) -> Vec<ChatEvent> {
    let mut expected = submission.tasks.len();
    let mut done = 0;
    let mut settled = submission.group.is_none();
    let mut events = Vec::new();
    while !settled || done < expected {
        let event = rx.recv_timeout(Duration::from_secs(45)).unwrap();
        if event.kind == ChatEventKind::Admitted {
            expected += 1;
        }
        if event.kind == ChatEventKind::GroupSettled {
            settled = true;
        }
        if matches!(
            event.kind,
            ChatEventKind::Completed | ChatEventKind::Failed | ChatEventKind::Cancelled
        ) {
            done += 1;
        }
        events.push(event);
    }
    events
}
pub fn configure(core: &Core, persona: Option<&str>, url: &str, api: &str, profile: &str) {
    let mut s = core.classifier_settings(persona).unwrap();
    s.connection.enabled = true;
    s.connection.model = "synthetic-participation".into();
    s.connection.base_url = url.into();
    s.connection.api = api.into();
    s.request_profile = profile.into();
    core.save_classifier_settings(persona, &s, &pebble_core::CredentialChange::Clear)
        .unwrap();
}
pub fn wait(mut ready: impl FnMut() -> bool) {
    for _ in 0..1000 {
        if ready() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("bounded classifier fixture timed out");
}
