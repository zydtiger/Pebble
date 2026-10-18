mod live_support;
mod responses_support;
use live_support::*;
use pebble_core::ChatEventKind;
use serde_json::{Value, json};
use std::time::Duration;

fn wire(api: &str, value: &Value) -> String {
    if api == "responses" {
        responses_support::candidate(&value.to_string())
    } else {
        candidate(&value.to_string())
    }
}
fn memory(api: &str) -> String {
    let args = json!({"content":"Synthetic preference: blue."}).to_string();
    if api == "responses" {
        responses_support::completed(&[
            json!({"type":"function_call","id":"item-memory","call_id":"call-memory","name":"update_memory","arguments":args,"status":"completed"}),
        ])
    } else {
        format!(
            "{}data: [DONE]\n\n",
            event(
                json!({"tool_calls":[{"index":0,"id":"call-memory","type":"function","function":{"name":"update_memory","arguments":args}}]}),
                json!("tool_calls")
            )
        )
    }
}

#[test]
fn intent_repair_or_rejection_preserves_chat_memory_and_pending_work() {
    for api in ["chat_completions", "responses"] {
        for repair in ["corrected", "invalid", "transport", "malformed"] {
            let invalid = json!({"reply":"Hello!","state":{"affinity_user":21,"mood":"calm"},"activity":{"objective":"Unaccepted objective","summary":null,"future":null,"next_wake_minutes":null,"intents":[{"action":"fulfill","id":"pending-intent","kind":"optional","summary":"Saved research","not_before_minutes":0,"condition":"activity_complete"}]}});
            let mut corrected = invalid.clone();
            corrected["activity"] = Value::Null;
            let mut plans = vec![stream(memory(api)), stream(wire(api, &invalid))];
            match repair {
                "corrected" => plans.push(stream(wire(api, &corrected))),
                "invalid" => {
                    plans.extend([stream(wire(api, &invalid)), stream(wire(api, &invalid))])
                }
                "transport" => {
                    let mut failed = stream(String::new());
                    failed.status = 503;
                    plans.push(failed);
                }
                "malformed" => {
                    plans.extend([stream(wire(api, &json!({}))), stream(wire(api, &json!({})))])
                }
                _ => unreachable!(),
            }
            let request_count = plans.len();
            plans.push(stream(wire(api, &corrected)));
            let server = Server::memory(plans);
            let (root, core, id) = isolated();
            configure(root.path(), &server.url, "mock", api, "");
            let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
            // Keep autonomous checks quiet while exercising the ordinary chat worker.
            let minute: u16 = db.query_row("SELECT CAST(strftime('%H','now','localtime') AS INTEGER)*60+CAST(strftime('%M','now','localtime') AS INTEGER)", [], |r|r.get(0)).unwrap();
            let mut settings = core.activity_settings(&id).unwrap();
            settings.enabled = true;
            settings.quiet_start_minutes = Some(minute);
            settings.quiet_end_minutes = Some((minute + 10) % 1440);
            core.save_activity_settings(&id, &settings).unwrap();
            let room = core.workspace().unwrap().personas[0]
                .conversation_id
                .clone();
            db.execute("INSERT INTO share_intents(id,activity_id,kind,destination_id,summary,condition,status) SELECT 'pending-intent',id,'optional',?2,'Saved research','activity_complete','pending' FROM persona_activities WHERE persona_id=?1", rusqlite::params![id,room]).unwrap();
            let objective: String = db
                .query_row(
                    "SELECT objective FROM persona_activities WHERE persona_id=?1",
                    [&id],
                    |r| r.get(0),
                )
                .unwrap();
            let (_task, rx) = start(&core, &id, "hi");
            let events = terminal(&rx);
            assert_eq!(
                events.last().unwrap().kind,
                ChatEventKind::Completed,
                "{api}/{repair}: {events:?}"
            );
            assert_eq!(
                core.messages(&id).unwrap().last().unwrap().content,
                "Hello!"
            );
            assert_eq!(
                core.workspace().unwrap().personas[0].memory,
                "Synthetic preference: blue."
            );
            assert_eq!(
                db.query_row(
                    "SELECT status FROM share_intents WHERE id='pending-intent'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "pending"
            );
            assert_eq!(
                db.query_row(
                    "SELECT objective FROM persona_activities WHERE persona_id=?1",
                    [&id],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                objective
            );
            assert_eq!(db.query_row("SELECT json_extract(current_state,'$.affinity_user') FROM persona_states WHERE persona_id=?1",[&id],|r|r.get::<_,i64>(0)).unwrap(),21);
            let mut requests = Vec::new();
            for _ in 0..request_count {
                requests.push(
                    server
                        .requests
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap()
                        .1,
                );
            }
            assert!(requests[0].to_string().contains("eligible_for_fulfillment"));
            assert!(requests[2].to_string().contains("Activity update rejected"));
            for request in &requests[2..] {
                assert_eq!(request["tool_choice"], "none");
            }
            let (_task, rx) = start(&core, &id, "Continue");
            assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
            let replay = server
                .requests
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .1
                .to_string();
            assert_eq!(replay.contains("activity_feedback"), repair != "corrected");
            assert!(replay.contains("call-memory"));
            assert_eq!(
                db.query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
}
