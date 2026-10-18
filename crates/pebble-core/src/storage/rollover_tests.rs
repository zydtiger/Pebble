use super::*;
use crate::{Core, ModelRequest};
use serde_json::{Value, json};

pub(super) fn persona(core: &Core, api: &str) -> Persona {
    let mut p = core.workspace().unwrap().personas.remove(0);
    p.chat = Some(crate::ChatConnection {
        model: "test".into(),
        base_url: "http://127.0.0.1:9/v1".into(),
        api: api.into(),
        context_len: 65536,
        max_output_tokens: 512,
        api_key: None,
        api_key_env: None,
        responses_schema_format: None,
        request: Default::default(),
    });
    p.context = p.chat.as_ref().unwrap().budget();
    p
}
pub(super) fn complete(db: &Database, p: &Persona, t: &crate::history::Turn, text: &str) {
    let answer = json!({"reply":text,"state":p.initial_state}).to_string();
    let out = if p.chat.as_ref().unwrap().api == "responses" {
        json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":answer}]})
    } else {
        json!({"role":"assistant","content":answer})
    };
    db.finish_turn(t, text, MessageStatus::Complete, None, Some(&[out]))
        .unwrap();
}
pub(super) fn events(r: &ModelRequest) -> Vec<Value> {
    r.items
        .iter()
        .filter_map(|i| serde_json::from_slice::<Value>(&i.payload).ok())
        .filter_map(|v| {
            v["content"]
                .as_str()
                .or_else(|| v["content"][0]["text"].as_str())
                .and_then(|t| serde_json::from_str::<Value>(t).ok())
        })
        .flat_map(|v| v["events"].as_array().cloned().unwrap_or_default())
        .collect()
}
#[test]
fn rollover_preserves_exact_unseen_holes_cross_room_observations_and_restart() {
    for api in ["chat_completions", "responses"] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let p = persona(&core, api);
        let db = &core.database;
        let group = core
            .create_group("room", std::slice::from_ref(&p.id))
            .unwrap();
        let a = db.enqueue_turn(&p, "first seen").unwrap();
        let a = db.activate_turn(&p, &a).unwrap();
        let pending = db.enqueue_turn(&p, "queued unseen private").unwrap();
        let group_user = db
            .enqueue_room(&group.id, "unseen group", &[], std::slice::from_ref(&p))
            .unwrap()
            .0;
        complete(db, &p, &a, "accepted output after queued inputs");
        let job = db.begin_context_maintenance(&p, true).unwrap();
        let frozen = serde_json::to_string(&job.request).unwrap();
        assert!(!frozen.contains("queued unseen private"));
        assert!(!frozen.contains("unseen group"));
        db.publish_maintenance(&job, &p).unwrap();
        let b = db.activate_turn(&p, &pending).unwrap();
        let input = db.request(&b.call_id).unwrap();
        let seen = events(&input);
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0]["message_id"], pending.user.id);
        complete(db, &p, &b, "second");
        // A later receipt catches the silent group event once, without replaying summarized input.
        let c = db.enqueue_turn(&p, "third").unwrap();
        let c = db.activate_turn(&p, &c).unwrap();
        let input = db.request(&c.call_id).unwrap();
        let seen = events(&input);
        assert_eq!(
            seen.iter()
                .filter(|e| e["message_id"] == group_user.id)
                .count(),
            1
        );
        assert!(!seen.iter().any(|e| e["message_id"] == a.user.id));
        complete(db, &p, &c, "third");
        drop(core);
        let core = Core::open(root.path()).unwrap();
        let replay = core.model_request(&c.call_id).unwrap();
        assert_eq!(
            replay.items.iter().map(|i| &i.payload).collect::<Vec<_>>(),
            input.items.iter().map(|i| &i.payload).collect::<Vec<_>>()
        );
    }
}

#[test]
fn rollover_preserves_unactivated_steered_inputs_once() {
    for api in ["chat_completions", "responses"] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let p = persona(&core, api);
        let db = &core.database;
        let original = db.enqueue_turn(&p, "already consumed").unwrap();
        let original = db.activate_turn(&p, &original).unwrap();
        complete(db, &p, &original, "accepted source");
        let send = |text| {
            db.enqueue_input(
                std::slice::from_ref(&p),
                crate::chat::RoomInput {
                    room: &p.conversation_id,
                    text,
                    mentions: &[],
                    images: &[],
                    separate_tasks: false,
                    steer: true,
                },
            )
            .unwrap()
            .1
            .remove(0)
        };
        let first = send("unactivated original request");
        let latest = send("new steering constraint");
        assert!(!db.call_active(&first.call_id));
        let job = db.begin_context_maintenance(&p, true).unwrap();
        let review = serde_json::to_string(&job.request).unwrap();
        assert!(!review.contains("unactivated original request"));
        assert!(!review.contains("new steering constraint"));
        db.publish_maintenance(&job, &p).unwrap();
        let latest = db.activate_turn(&p, &latest).unwrap();
        let request = db.request(&latest.call_id).unwrap();
        let observed = events(&request);
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0]["message_id"], first.user.id);
        assert_eq!(observed[1]["message_id"], latest.user.id);
        assert_ne!(request.session_id, original.session_id);
        complete(db, &p, &latest, "current answer");
        drop(core);
        let core = Core::open(root.path()).unwrap();
        let replay = core.model_request(&latest.call_id).unwrap();
        assert_eq!(events(&replay), observed);
        assert_eq!(replay.history_cutoff, request.history_cutoff);
    }
}

#[test]
fn schema_15_steering_migrates_without_rewriting_native_context() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let p = persona(&core, "chat_completions");
    let db = &core.database;
    let turn = db.enqueue_turn(&p, "saved main context").unwrap();
    let turn = db.activate_turn(&p, &turn).unwrap();
    complete(db, &p, &turn, "saved main answer");
    let before = db.request(&turn.call_id).unwrap();
    let pending = db.enqueue_turn(&p, "saved pending input").unwrap();
    db.connection
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO steered_calls(call_id,user_id) VALUES(?1,?2)",
            params![pending.call_id, turn.user.id],
        )
        .unwrap();
    drop(core);
    let conn = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
    conn.execute_batch("DROP TABLE activity_delegation_submissions; DROP TABLE activity_calls; DROP TABLE activity_steps; DROP TABLE activity_events; DROP TABLE share_intents; DROP TABLE persona_activities; DROP TABLE persona_activity_runtime; DROP INDEX model_items_activity_call; ALTER TABLE model_items DROP COLUMN activity_call_id; ALTER TABLE model_sessions DROP COLUMN private_activity_contract; ALTER TABLE delegated_tasks DROP COLUMN activity_step_id; ALTER TABLE delegated_tasks DROP COLUMN activity_id; ALTER TABLE delegated_tasks DROP COLUMN delivery; ALTER TABLE delegated_tasks DROP COLUMN origin; DROP TABLE session_recent_messages; DROP TABLE automatic_rollovers; DROP TABLE summarized_events; ALTER TABLE context_maintenance DROP COLUMN automatic; ALTER TABLE model_calls DROP COLUMN history_cutoff; ALTER TABLE model_sessions DROP COLUMN history_tools; PRAGMA user_version=15;").unwrap();
    let core = Core::open(root.path()).unwrap();
    let after = core.model_request(&turn.call_id).unwrap();
    assert_eq!(after.session_id, before.session_id);
    assert!(!after.history_tools);
    assert_eq!(
        after.items.iter().map(|i| &i.payload).collect::<Vec<_>>(),
        before.items.iter().map(|i| &i.payload).collect::<Vec<_>>()
    );
    assert_eq!(
        core.database.steered_calls(&turn.user.id).unwrap(),
        [pending.call_id]
    );
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        21
    );
    assert!(
        !conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
}
#[test]
fn thresholds_respect_idle_backlog_and_priority_projection() {
    let budget = crate::ContextBudget {
        context_len: 65536,
        max_output_tokens: 512,
    };
    let c = budget.chat_ceiling();
    assert!(!budget.rollover_due(c * 8 / 10 - 1, false));
    assert!(budget.rollover_due(c * 8 / 10, false));
    assert!(!budget.rollover_due(c * 8 / 10, true));
    assert!(budget.rollover_due(c * 9 / 10, true));
    for busy in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let mut p = persona(&core, "chat_completions");
        let db = &core.database;
        let a = db.enqueue_turn(&p, &"x".repeat(14000)).unwrap();
        let a = db.activate_turn(&p, &a).unwrap();
        complete(db, &p, &a, "done");
        let request = super::items::request(
            &db.connection.lock().unwrap(),
            root.path(),
            &a.call_id,
            true,
        )
        .unwrap();
        let estimate = crate::images::estimate(
            &crate::live::request_body(p.chat.as_ref().unwrap(), &request, 512)
                .unwrap()
                .1,
        )
        .unwrap();
        // Adjust only the local budget so this exact request sits between thresholds.
        p.context.context_len = (estimate * 100 / 85 + 512) * 3 / 2;
        let pending = busy.then(|| db.enqueue_turn(&p, "pending").unwrap());
        let job = db.automatic_rollover(&p, None).unwrap();
        assert_eq!(job.is_some(), !busy);
        if let Some(job) = job {
            db.finish_maintenance(&job.id, "failed", "fixture").unwrap();
            assert!(db.automatic_rollover(&p, None).unwrap().is_none());
        }
        if let Some(pending) = pending {
            // A known large next request gets maintenance even with backlog.
            let dbconn = db.connection.lock().unwrap();
            dbconn
                .execute(
                    "UPDATE messages SET content=?2 WHERE id=?1",
                    params![pending.user.id, "y".repeat(16000)],
                )
                .unwrap();
            drop(dbconn);
            let job = db.automatic_rollover(&p, Some(&pending)).unwrap().unwrap();
            db.finish_maintenance(&job.id, "failed", "fixture").unwrap();
            assert!(db.automatic_rollover(&p, Some(&pending)).unwrap().is_none());
        }
    }
}
#[test]
fn interrupted_automatic_attempt_does_not_restart_and_manual_recovers_saved_review() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    let mut p = persona(&core, "chat_completions");
    let db = &core.database;
    let a = db.enqueue_turn(&p, &"x".repeat(20000)).unwrap();
    let a = db.activate_turn(&p, &a).unwrap();
    complete(db, &p, &a, "done");
    p.context.context_len = 55000;
    let job = db.automatic_rollover(&p, None).unwrap().unwrap();
    let call = crate::memory::Call {
        id: "review".into(),
        arguments: json!({"content":job.memory}).to_string(),
        content: job.memory.clone(),
    };
    db.execute_memory(
        super::memory::MemoryContext {
            owner: &job.id,
            persona: &p,
            session: None,
            protocol: "chat_completions",
            expected: &job.memory,
        },
        &[],
        &call,
        &crate::chat::Control::default(),
    )
    .unwrap();
    let id = job.id.clone();
    drop(core);
    let core = Core::open(root.path()).unwrap();
    assert!(
        core.database
            .automatic_rollover(&p, None)
            .unwrap()
            .is_none()
    );
    let recovered = core.database.begin_maintenance(&p).unwrap();
    assert_eq!(recovered.id, id);
    assert!(recovered.automatic);
    core.database.publish_maintenance(&recovered, &p).unwrap();
    assert_eq!(core.context_markers(&p.conversation_id).unwrap().len(), 1);
}
