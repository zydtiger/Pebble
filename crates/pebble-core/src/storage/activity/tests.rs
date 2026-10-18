use super::*;
use crate::{MessageStatus, ModelRequest};

#[test]
fn core_drop_waits_for_scheduler_tick_before_releasing_database() {
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    let root = tempfile::tempdir().unwrap();
    let core = crate::Core::open(root.path()).unwrap();
    let database = core.database.clone();
    let connection = database.connection.lock().unwrap();
    // Hold the database until the real scheduler has entered a tick. Its
    // temporary Manager reference cannot leave that tick while this lock is held.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Arc::strong_count(&core.activity) == 1 {
        assert!(Instant::now() < deadline, "scheduler did not enter a tick");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (finished, receiver) = mpsc::channel();
    let closing = std::thread::spawn(move || {
        drop(core);
        finished.send(()).unwrap();
    });
    assert!(matches!(
        receiver.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(connection);
    drop(database);
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    closing.join().unwrap();
    let reopened = crate::Core::open(root.path()).unwrap();
    drop(reopened);
}

fn fixture(api: &str) -> (tempfile::TempDir, Database, Persona) {
    let root = tempfile::tempdir().unwrap();
    crate::config::initialize(root.path()).unwrap();
    let db = Database::open(root.path()).unwrap();
    let mut workspace = crate::config::load(root.path()).unwrap();
    db.resolve_conversations(&mut workspace.personas).unwrap();
    let mut p = workspace.personas.remove(0);
    p.chat = Some(crate::ChatConnection {
        model: "test".into(),
        base_url: "http://127.0.0.1:9/v1".into(),
        api: api.into(),
        context_len: 262144,
        max_output_tokens: 512,
        api_key: None,
        api_key_env: None,
        responses_schema_format: None,
        request: Default::default(),
    });
    p.context = p.chat.as_ref().unwrap().budget();
    (root, db, p)
}
fn enable(db: &Database, p: &Persona) {
    db.save_activity_settings(&p.id, true, None, None, 24)
        .unwrap();
}
fn due(db: &Database, p: &Persona) {
    db.connection.lock().unwrap().execute("UPDATE persona_activity_runtime SET next_wake_at='2000-01-01T00:00:00Z' WHERE persona_id=?1",[&p.id]).unwrap();
}
fn wake(db: &Database, p: &Persona) -> ModelRequest {
    due(db, p);
    let step = db
        .claim_due_activity(&p.id)
        .unwrap()
        .expect("one admitted wake");
    db.activate_activity_call(p, &step).unwrap()
}
fn native(p: &Persona, value: Value) -> Vec<Value> {
    if p.chat.as_ref().unwrap().api == "responses" {
        vec![
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":value.to_string()}]}),
        ]
    } else {
        vec![json!({"role":"assistant","content":value.to_string()})]
    }
}
fn quiet(p: &Persona) -> Value {
    json!({"state":p.initial_state,"activity":{"summary":"Reviewing privately.","phase":"waiting","next_wake_minutes":45,"plans":null,"share_intent_id":null}})
}
fn reviewed(p: &Persona, request: &ModelRequest) -> Value {
    let mut output = quiet(p);
    output["activity"]["observed_outcomes"] = envelope(request)["task_outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| json!({"task_id":o["task_id"],"outcome_revision":o["outcome_revision"]}))
        .collect();
    output
}
fn envelope(r: &ModelRequest) -> Value {
    let v: Value = serde_json::from_slice(&r.items.last().unwrap().payload).unwrap();
    serde_json::from_str(
        v["content"]
            .as_str()
            .or_else(|| v["content"][0]["text"].as_str())
            .unwrap(),
    )
    .unwrap()
}
fn count(db: &Database, table: &str) -> i64 {
    db.connection
        .lock()
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn proposal(ops: Vec<Value>) -> Value {
    json!({"objective":null,"summary":null,"future":null,"next_wake_minutes":null,"intents":ops})
}
fn create(kind: &str) -> Value {
    json!({"action":"create","id":null,"kind":kind,"summary":"Return with an interesting finding","not_before_minutes":0,"condition":"ready"})
}
fn reply(db: &Database, p: &Persona, metadata: Value) -> String {
    let turn = db.enqueue_turn(p, "Hello").unwrap();
    let turn = db.activate_turn(p, &turn).unwrap();
    db.finish_turn(
        &turn,
        "I will come back",
        MessageStatus::Complete,
        None,
        Some(&native(
            p,
            json!({"reply":"I will come back","state":p.initial_state,"activity":metadata}),
        )),
    )
    .unwrap();
    turn.reply.id
}
fn intent(db: &Database) -> String {
    db.connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT id FROM share_intents WHERE status='pending' ORDER BY rowid LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn private_turns_share_history_but_never_publish_messages_or_contact_time() {
    for api in ["chat_completions", "responses"] {
        let (_root, db, p) = fixture(api);
        enable(&db, &p);
        let first = reply(&db, &p, Value::Null);
        let before = count(&db, "messages");
        let events = count(&db, "persona_events");
        let request = wake(&db, &p);
        let frozen = envelope(&request)["temporal_context"].clone();
        assert!(frozen["last_user_contact_at"].is_string());
        assert!(frozen["last_published_persona_reply_at"].is_string());
        assert!(frozen["current_user_receipt_at"].is_null());
        assert!(
            request.response_schema.as_ref().unwrap()["properties"]
                .get("reply")
                .is_none()
        );
        assert!(request.delegation_tools);
        db.finish_activity_call(&request, "completed", None, Some(&native(&p, quiet(&p))))
            .unwrap();
        assert_eq!(count(&db, "messages"), before);
        assert_eq!(count(&db, "persona_events"), events);
        let next = wake(&db, &p);
        assert_eq!(request.session_id, next.session_id);
        assert_eq!(
            envelope(&next)["temporal_context"]["last_published_persona_reply_at"],
            frozen["last_published_persona_reply_at"]
        );
        assert!(next.items.len() > request.items.len());
        db.finish_activity_call(&next, "cancelled", None, None)
            .unwrap();
        assert_eq!(
            db.activity_snapshot(&p.id).unwrap().last_publication_id,
            None
        );
        assert!(!first.is_empty());
    }
}

#[test]
fn commitments_commit_with_publication_and_share_fulfills_once() {
    for api in ["chat_completions", "responses"] {
        let (_root, db, p) = fixture(api);
        enable(&db, &p);
        let promise = reply(&db, &p, proposal(vec![create("commitment")]));
        let id = intent(&db);
        let saved: String = db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT promise_message_id FROM share_intents WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved, promise);
        let request = wake(&db, &p);
        let mut output = quiet(&p);
        output["activity"]["share_intent_id"] = json!(id);
        db.finish_activity_call(&request, "completed", None, Some(&native(&p, output)))
            .unwrap();
        assert_eq!(count(&db, "messages"), 2);
        let share = db
            .begin_activity_share(&p, &request.call_id)
            .unwrap()
            .unwrap();
        assert_eq!(count(&db, "messages"), 2); // No placeholder.
        assert!(!share.delegation_tools);
        assert_eq!(share.session_id, request.session_id);
        let output = native(
            &p,
            json!({"reply":"I found something worth sharing.","state":p.initial_state}),
        );
        db.finish_activity_share(&share, Some(&output), None)
            .unwrap();
        db.finish_activity_share(&share, Some(&output), None)
            .unwrap();
        assert_eq!(count(&db, "messages"), 3);
        let snapshot = db.activity_snapshot(&p.id).unwrap();
        let (status, message): (String, String) = db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT status,fulfillment_message_id FROM share_intents WHERE id=?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "fulfilled");
        assert_eq!(snapshot.last_publication_id, Some(message));
        assert!(
            db.begin_activity_share(&p, &request.call_id)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn intent_caps_and_failed_outputs_roll_back_all_plan_changes() {
    let (_root, db, p) = fixture("responses");
    enable(&db, &p);
    reply(
        &db,
        &p,
        proposal((0..3).map(|_| create("commitment")).collect()),
    );
    let turn = db.enqueue_turn(&p, "Another promise?").unwrap();
    let turn = db.activate_turn(&p, &turn).unwrap();
    let output = native(
        &p,
        json!({"reply":"Too many","state":p.initial_state,"activity":proposal(vec![create("optional"),create("commitment")])}),
    );
    db.finish_turn(
        &turn,
        "Too many",
        MessageStatus::Complete,
        None,
        Some(&output),
    )
    .unwrap();
    assert_eq!(count(&db, "share_intents"), 3);
    let request = wake(&db, &p);
    let mut output = quiet(&p);
    output["activity"]["plans"] = proposal(vec![create("commitment")]);
    assert!(
        db.finish_activity_call(&request, "completed", None, Some(&native(&p, output)))
            .is_err()
    );
    db.finish_activity_call(&request, "failed", Some("invalid"), None)
        .unwrap();
    assert_eq!(count(&db, "share_intents"), 3);
    let request = wake(&db, &p);
    let mut output = quiet(&p);
    output["activity"]["plans"] = proposal((0..5).map(|_| create("optional")).collect());
    db.finish_activity_call(&request, "completed", None, Some(&native(&p, output)))
        .unwrap();
    let request = wake(&db, &p);
    let mut output = quiet(&p);
    output["activity"]["plans"] = proposal(vec![create("optional")]);
    assert!(
        db.finish_activity_call(&request, "completed", None, Some(&native(&p, output)))
            .is_err()
    );
    assert_eq!(count(&db, "share_intents"), 8);
}

#[test]
fn queued_user_or_disable_prevents_return_and_stop_preserves_control() {
    for disable in [false, true] {
        let (_root, db, p) = fixture("chat_completions");
        enable(&db, &p);
        reply(&db, &p, proposal(vec![create("commitment")]));
        let request = wake(&db, &p);
        let mut output = quiet(&p);
        output["activity"]["share_intent_id"] = json!(intent(&db));
        db.finish_activity_call(&request, "completed", None, Some(&native(&p, output)))
            .unwrap();
        let share = db
            .begin_activity_share(&p, &request.call_id)
            .unwrap()
            .unwrap();
        if disable {
            db.save_activity_settings(&p.id, false, None, None, 24)
                .unwrap();
        } else {
            db.enqueue_turn(&p, "Wait, talk to me first").unwrap();
        }
        db.finish_activity_share(
            &share,
            Some(&native(
                &p,
                json!({"reply":"Late discovery","state":p.initial_state}),
            )),
            None,
        )
        .unwrap();
        assert!(
            db.activity_snapshot(&p.id)
                .unwrap()
                .last_publication_id
                .is_none()
        );
        assert_eq!(count(&db, "share_intents"), 1);
        assert!(!intent(&db).is_empty());
        db.stop_activity(&p.id).unwrap();
        assert!(!db.activity_snapshot(&p.id).unwrap().enabled);
    }
}

#[test]
fn pending_group_receipt_rejects_private_effects_before_reply_admission() {
    receipt_rejects_private_effects(true);
}

#[test]
fn silent_group_receipt_rejects_private_effects_before_queue_cancellation() {
    receipt_rejects_private_effects(false);
}

fn receipt_rejects_private_effects(mentioned: bool) {
    let (_root, db, p) = fixture("responses");
    let room = db
        .create_group("Waiting group", std::slice::from_ref(&p.id))
        .unwrap();
    enable(&db, &p);
    let request = wake(&db, &p);
    let state_before = db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT current_state FROM persona_states WHERE persona_id=?1",
            [&p.id],
            |r| r.get::<_, String>(0),
        )
        .unwrap();
    let (_, turns, _) = db
        .enqueue_input(
            std::slice::from_ref(&p),
            crate::chat::RoomInput {
                room: &room.id,
                text: "Answer here",
                mentions: if mentioned {
                    std::slice::from_ref(&p.id)
                } else {
                    &[]
                },
                images: &[],
                separate_tasks: false,
                steer: false,
            },
        )
        .unwrap();
    assert!(turns.is_empty());
    assert_eq!(count(&db, "model_calls"), 0);
    let call = crate::codex::tool::Call {
        id: "late-call".into(),
        arguments: crate::codex::tool::Arguments {
            title: "Research".into(),
            instructions: "Research".into(),
            request_key: "fermentation-curiosity".into(),
            task_id: None,
        },
    };
    assert!(
        db.prepare_activity_delegation(&request, &call, &[], &crate::CodexSettings::default())
            .is_err()
    );
    assert!(
        db.record_activity_tool_exchange(&request, &[], &json!({}))
            .is_err()
    );
    assert_eq!(count(&db, "delegated_tasks"), 0);
    let items = count(&db, "model_items");
    db.finish_activity_call(&request, "completed", None, Some(&native(&p, quiet(&p))))
        .unwrap();
    assert_eq!(count(&db, "model_items"), items);
    let conn = db.connection.lock().unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM activity_calls WHERE id=?1",
            [&request.call_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "cancelled");
    let state: String = conn
        .query_row(
            "SELECT current_state FROM persona_states WHERE persona_id=?1",
            [&p.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, state_before);
    drop(conn);
    due(&db, &p);
    assert_eq!(db.claim_due_activity(&p.id).unwrap().is_none(), mentioned);
}

#[test]
fn task_admission_is_idempotent_private_and_frozen_outcomes_only_are_acknowledged() {
    let (_root, db, p) = fixture("responses");
    enable(&db, &p);
    let request = wake(&db, &p);
    let call = crate::codex::tool::Call {
        id: "provider-1".into(),
        arguments: crate::codex::tool::Arguments {
            title: "Fermentation curiosity".into(),
            instructions: "Compare sourdough and kefir fermentation using public references."
                .into(),
            request_key: "fermentation-curiosity".into(),
            task_id: None,
        },
    };
    let settings = crate::CodexSettings::default();
    let (_, dispatch) = db
        .prepare_activity_delegation(&request, &call, &[], &settings)
        .unwrap();
    let id = dispatch.unwrap();
    assert!(
        db.prepare_activity_delegation(&request, &call, &[], &settings)
            .unwrap()
            .1
            .is_none()
    );
    assert_eq!(count(&db, "delegated_tasks"), 1);
    let task = db.task(&id).unwrap();
    assert_eq!(task.title, call.arguments.title);
    assert_eq!(task.instructions, call.arguments.instructions);
    assert_ne!(task.original_request, task.instructions);
    assert!(task.original_request.contains("enabled by the user"));
    assert_eq!(task.delivery, "private_observation");
    assert_eq!(task.settings.writable, settings.writable);
    db.finish_activity_call(&request, "cancelled", None, None)
        .unwrap();
    assert_eq!(db.task(&id).unwrap().state, DelegationState::Queued); // Chat cancellation leaves accepted work alive.
    db.update_task(&id, |t| {
        t.state = DelegationState::Completed;
        t.result = "Public discovery".into();
        Ok(())
    })
    .unwrap();
    assert!(db.queue_activity_task_outcome(&id).unwrap());
    assert!(!db.queue_activity_task_outcome(&id).unwrap());
    let review = wake(&db, &p);
    assert_eq!(
        envelope(&review)["task_outcomes"][0]["result"],
        "Public discovery"
    );
    db.update_task(&id, |t| {
        t.result = "Newer saved revision".into();
        Ok(())
    })
    .unwrap();
    db.queue_activity_task_outcome(&id).unwrap();
    db.finish_activity_call(
        &review,
        "completed",
        None,
        Some(&native(&p, reviewed(&p, &review))),
    )
    .unwrap();
    assert_eq!(db.task(&id).unwrap().report_state, "private_queued");
    let review = wake(&db, &p);
    assert_eq!(
        envelope(&review)["task_outcomes"][0]["result"],
        "Newer saved revision"
    );
    db.finish_activity_call(
        &review,
        "completed",
        None,
        Some(&native(&p, reviewed(&p, &review))),
    )
    .unwrap();
    assert_eq!(db.task(&id).unwrap().report_state, "private_assimilated");
    assert_eq!(count(&db, "messages"), 0);
    assert_eq!(count(&db, "persona_events"), 0);
}

#[test]
fn recovery_coalesces_wakes_and_daily_budget_and_quiet_hours_are_enforced() {
    let (root, db, p) = fixture("responses");
    assert!(!db.activity_settings(&p.id).unwrap().enabled);
    db.save_activity_settings(&p.id, true, None, None, 1)
        .unwrap();
    let request = wake(&db, &p);
    due(&db, &p);
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    drop(db);
    let db = Database::open(root.path()).unwrap();
    db.recover_activities().unwrap();
    due(&db, &p);
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    assert!(
        db.activity_settings(&p.id)
            .unwrap()
            .next_reason
            .contains("budget")
    );
    let status: String = db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT status FROM activity_calls WHERE id=?1",
            [request.call_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "interrupted");
    let minute = local_minutes() as u16;
    db.save_activity_settings(&p.id, true, Some(minute), Some((minute + 2) % 1440), 24)
        .unwrap();
    due(&db, &p);
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    assert_eq!(
        db.activity_settings(&p.id).unwrap().next_reason,
        "Quiet hours"
    );
    assert!(
        db.save_activity_settings(&p.id, true, Some(1440), Some(60), 24)
            .is_err()
    );
    db.save_activity_settings(&p.id, false, None, None, 24)
        .unwrap();
    due(&db, &p);
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    db.stop_activity(&p.id).unwrap();
    assert!(db.activity_snapshot(&p.id).unwrap().next_check_at.is_none());
}

#[test]
fn rollover_carries_last_ten_chat_ids_despite_many_private_turns() {
    for api in ["chat_completions", "responses"] {
        let (_root, db, p) = fixture(api);
        enable(&db, &p);
        for _ in 0..6 {
            reply(&db, &p, Value::Null);
        }
        let expected: Vec<String> = {
            let c = db.connection.lock().unwrap();
            c.prepare("SELECT message_id FROM persona_events WHERE persona_id=?1 ORDER BY sequence DESC LIMIT 10").unwrap().query_map([&p.id],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap()
        };
        for _ in 0..12 {
            let r = wake(&db, &p);
            db.finish_activity_call(&r, "completed", None, Some(&native(&p, quiet(&p))))
                .unwrap();
        }
        let job = db.begin_context_maintenance(&p, true).unwrap();
        db.publish_maintenance(&job, &p).unwrap();
        let request = wake(&db, &p);
        assert!(request.recent_history.is_some());
        let actual: Vec<String> = {
            let c = db.connection.lock().unwrap();
            c.prepare("SELECT message_id FROM session_recent_messages WHERE session_id=?1")
                .unwrap()
                .query_map([request.session_id], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(actual.len(), 10);
        for id in expected {
            assert!(actual.contains(&id));
        }
        assert_eq!(count(&db, "messages"), 12);
    }
}

#[test]
fn private_only_history_can_trigger_automatic_rollover() {
    let (_root, db, mut p) = fixture("responses");
    enable(&db, &p);
    let r = wake(&db, &p);
    db.finish_activity_call(&r, "completed", None, Some(&native(&p, quiet(&p))))
        .unwrap();
    {
        let mut c = db.connection.lock().unwrap();
        let tx = c.transaction().unwrap();
        insert_activity_item(&tx,&r.session_id,&r.call_id,&json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"large private evidence ".repeat(12000)}]})).unwrap();
        tx.commit().unwrap();
    }
    p.context.context_len = 65536;
    assert!(db.automatic_rollover(&p, None).unwrap().is_some());
}

#[test]
fn receipt_time_is_separate_from_queue_delay_and_retry_replays_the_frozen_clock() {
    let (_root, db, p) = fixture("responses");
    reply(&db, &p, Value::Null);
    db.connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE messages SET created_at='2000-01-01T00:00:00Z' WHERE sender_kind='user'",
            [],
        )
        .unwrap();
    let pending = db.enqueue_turn(&p, "Current receipt").unwrap();
    let active = db.activate_turn(&p, &pending).unwrap();
    let original = db.request(&active.call_id).unwrap();
    let time = envelope(&original)["temporal_context"].clone();
    assert_eq!(time["last_user_contact_at"], "2000-01-01T00:00:00Z");
    assert!(time["seconds_since_last_user_contact"].as_i64().unwrap() > 86400);
    assert!(time["queue_delay_seconds"].as_i64().unwrap() < 10);
    db.finish_turn(&active, "", MessageStatus::Failed, Some("fixture"), None)
        .unwrap();
    let retry = db.enqueue_retry(&p, &active.reply.id).unwrap();
    let retry = db.activate_turn(&p, &retry).unwrap();
    let replay = db.request(&retry.call_id).unwrap();
    assert_eq!(envelope(&replay)["temporal_context"], time);
}

#[test]
fn legacy_prefix_waits_for_fresh_boundary_without_rewriting_native_history() {
    let (_root, db, p) = fixture("responses");
    reply(&db, &p, Value::Null);
    let prefix: Vec<u8> = {
        let c = db.connection.lock().unwrap();
        c.execute("UPDATE model_sessions SET private_activity_contract=0", [])
            .unwrap();
        c.query_row(
            "SELECT payload FROM model_items WHERE kind='prefix'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    enable(&db, &p);
    let step = db.claim_due_activity(&p.id).unwrap().unwrap();
    assert!(db.activate_activity_call(&p, &step).is_err());
    assert_eq!(
        db.activity_settings(&p.id).unwrap().next_reason,
        "Fresh context required for private activity"
    );
    assert_eq!(count(&db, "activity_calls"), 0);
    let current: Vec<u8> = db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT payload FROM model_items WHERE kind='prefix'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current, prefix);
    let job = db.begin_maintenance(&p).unwrap();
    db.publish_maintenance(&job, &p).unwrap();
    assert_eq!(
        db.activity_settings(&p.id).unwrap().next_reason,
        "Private activity context ready"
    );
    assert!(
        db.activate_activity_call(&p, &db.claim_due_activity(&p.id).unwrap().unwrap())
            .is_ok()
    );
}

mod settings_tests;

mod handover_tests;
