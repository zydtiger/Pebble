//! Shared fixtures used in different combinations by the group integration tests.
#![allow(dead_code)]
use pebble_core::{ChatEvent, ChatEventKind, ChatListener, ChatSubmission, Core, ModelRequest};
use serde_json::Value;
use std::sync::{Arc, mpsc};
use std::time::Duration;
struct Listener(mpsc::Sender<ChatEvent>);
impl ChatListener for Listener {
    fn on_event(&self, e: ChatEvent) -> Result<(), String> {
        self.0.send(e).map_err(|e| e.to_string())
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
pub fn listener() -> Arc<dyn ChatListener> {
    let (tx, _) = mpsc::channel();
    Arc::new(Listener(tx))
}
pub fn finish(rx: &mpsc::Receiver<ChatEvent>, count: usize) -> Vec<ChatEvent> {
    let mut events = Vec::new();
    let mut done = 0;
    while done < count {
        let e = rx.recv_timeout(Duration::from_secs(180)).unwrap();
        if matches!(
            e.kind,
            ChatEventKind::Completed | ChatEventKind::Failed | ChatEventKind::Cancelled
        ) {
            done += 1;
        }
        events.push(e);
    }
    events
}
pub fn envelope(request: &ModelRequest) -> Value {
    let input: Value = serde_json::from_slice(&request.items.last().unwrap().payload).unwrap();
    serde_json::from_str(if request.protocol == "responses" {
        input["content"][0]["text"].as_str().unwrap()
    } else {
        input["content"].as_str().unwrap()
    })
    .unwrap()
}
pub fn native(request: &ModelRequest) -> String {
    request
        .items
        .iter()
        .map(|i| String::from_utf8_lossy(&i.payload).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn db(root: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap()
}
pub fn count(db: &rusqlite::Connection, sql: &str) -> i64 {
    db.query_row(sql, [], |r| r.get(0)).unwrap()
}
