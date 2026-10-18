use super::*;
use crate::activity::settings::Heartbeat;

fn edit(root: &std::path::Path, id: &str, f: impl FnOnce(&mut toml_edit::DocumentMut)) {
    let path = policy::path(root, id).unwrap();
    let mut doc = crate::settings::document(&path).unwrap();
    f(&mut doc);
    std::fs::write(path, doc.to_string()).unwrap();
}

#[test]
fn controls_round_trip_through_toml_without_other_file_changes() {
    let (root, db, p) = fixture("responses");
    let path = policy::path(root.path(), &p.id).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    db.save_activity_settings(&p.id, false, Some(1380), Some(480), 9)
        .unwrap();
    let policy = policy::read(root.path(), &p.id).unwrap();
    assert_eq!(
        policy,
        Heartbeat::from_controls(false, Some(1380), Some(480), 9)
    );
    let mut original = before.parse::<toml_edit::DocumentMut>().unwrap();
    let mut saved = crate::settings::document(&path).unwrap();
    assert_eq!(saved["heartbeat"]["quiet_start"].as_str(), Some("23:00"));
    original.remove("heartbeat");
    saved.remove("heartbeat");
    assert_eq!(original.to_string(), saved.to_string());
    let state = db.activity_settings(&p.id).unwrap();
    assert!(!state.enabled);
    assert_eq!(state.daily_wake_budget, 9);
    db.save_activity_settings(&p.id, true, None, None, 9)
        .unwrap();
    assert!(
        !crate::settings::document(&path).unwrap()["heartbeat"]
            .as_table_like()
            .unwrap()
            .contains_key("quiet_start")
    );
    db.stop_activity(&p.id).unwrap();
    assert!(!policy::read(root.path(), &p.id).unwrap().enabled);
    db.start_activity(&p.id, "Explore a new question").unwrap();
    assert!(policy::read(root.path(), &p.id).unwrap().active());
}

#[test]
fn file_edits_enable_disable_and_change_budget_without_reloading_or_resetting_usage() {
    let (root, db, p) = fixture("responses");
    edit(root.path(), &p.id, |doc| {
        doc["heartbeat"]["enabled"] = toml_edit::value(true);
        doc["heartbeat"]["daily_wake_budget"] = toml_edit::value(1);
        let table = doc["heartbeat"].as_table_like_mut().unwrap();
        table.remove("quiet_start");
        table.remove("quiet_end");
    });
    let step = db
        .claim_due_activity(&p.id)
        .unwrap()
        .expect("file enabled first wake");
    db.finish_quiet_activity_step(&p.id, &step).unwrap();
    due(&db, &p);
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    assert!(
        db.activity_settings(&p.id)
            .unwrap()
            .next_reason
            .contains("budget")
    );
    edit(root.path(), &p.id, |doc| {
        doc["heartbeat"]["daily_wake_budget"] = toml_edit::value(2)
    });
    // Raising the cap rechecks the budget hold immediately, not tomorrow.
    let step = db
        .claim_due_activity(&p.id)
        .unwrap()
        .expect("raised file budget");
    edit(root.path(), &p.id, |doc| {
        doc["heartbeat"]["enabled"] = toml_edit::value(false)
    });
    assert!(db.activate_activity_call(&p, &step).is_err());
    db.finish_quiet_activity_step(&p.id, &step).unwrap();
    let settings = db.activity_settings(&p.id).unwrap();
    assert!(!settings.enabled);
    drop(db);
    let db = Database::open(root.path()).unwrap();
    edit(root.path(), &p.id, |doc| {
        doc["heartbeat"]["enabled"] = toml_edit::value(true)
    });
    due(&db, &p);
    assert!(
        db.claim_due_activity(&p.id).unwrap().is_none(),
        "restart preserves consumed budget"
    );
    let used: i64 = db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT wakes_used FROM persona_activity_runtime WHERE persona_id=?1",
            [&p.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(used, 2);
}

#[test]
fn file_quiet_hours_block_wakes_and_late_private_output_is_rejected_after_disable() {
    let (root, db, p) = fixture("responses");
    enable(&db, &p);
    let minute = local_minutes() as u16;
    let quiet = Heartbeat::from_controls(true, Some(minute), Some((minute + 2) % 1440), 24);
    policy::write(root.path(), &p.id, &quiet).unwrap();
    due(&db, &p);
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    assert_eq!(
        db.activity_settings(&p.id).unwrap().next_reason,
        "Quiet hours"
    );
    edit(root.path(), &p.id, |doc| {
        let table = doc["heartbeat"].as_table_like_mut().unwrap();
        table.remove("quiet_start");
        table.remove("quiet_end");
    });
    let step = db
        .claim_due_activity(&p.id)
        .unwrap()
        .expect("quiet hold lifted by file edit");
    let request = db.activate_activity_call(&p, &step).unwrap();
    edit(root.path(), &p.id, |doc| {
        doc["heartbeat"]["enabled"] = toml_edit::value(false)
    });
    db.finish_activity_call(
        &request,
        "completed",
        None,
        Some(&native(&p, super::quiet(&p))),
    )
    .unwrap();
    let status: String = db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT status FROM activity_calls WHERE id=?1",
            [&request.call_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "cancelled");
    assert!(!db.activity_snapshot(&p.id).unwrap().enabled);
}

#[test]
fn invalid_file_policy_is_reported_and_invalid_ui_save_preserves_the_file() {
    let (root, db, p) = fixture("responses");
    let path = policy::path(root.path(), &p.id).unwrap();
    let original = std::fs::read_to_string(&path).unwrap();
    for (start, end, budget) in [
        (Some(1440), Some(480), 4),
        (Some(480), None, 4),
        (Some(480), Some(480), 4),
        (None, None, 0),
        (None, None, 25),
    ] {
        assert!(
            db.save_activity_settings(&p.id, true, start, end, budget)
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
    for text in [
        "[heartbeat]\ndaily_wake_budget = 25\n",
        "[heartbeat]\nquiet_start = '8:00'\nquiet_end = '23:00'\n",
        "[heartbeat]\nquiet_start = '08:00'\n",
        "[heartbeat]\nunknown = true\n",
    ] {
        let mut doc = original.parse::<toml_edit::DocumentMut>().unwrap();
        doc.remove("heartbeat");
        std::fs::write(&path, format!("{doc}\n{text}")).unwrap();
        assert!(crate::config::load(root.path()).is_err());
        assert!(db.claim_due_activity(&p.id).is_err());
    }
}

#[test]
fn schema_twenty_migrates_policy_once_preserving_runtime_and_explicit_files() {
    for existing_file_policy in [false, true] {
        let (root, db, p) = fixture("responses");
        // Reconstruct the exact prior settings table around existing runtime.
        db.connection
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE persona_activity_runtime;")
            .unwrap();
        let sql = include_str!("../../activity-migration.sql")
            .split("CREATE TABLE persona_activities")
            .next()
            .unwrap();
        db.connection.lock().unwrap().execute_batch(sql).unwrap();
        db.connection.lock().unwrap().execute_batch("CREATE INDEX activity_due ON persona_activity_settings(enabled,paused,next_wake_at); PRAGMA user_version=20;").unwrap();
        db.connection.lock().unwrap().execute("INSERT INTO persona_activity_settings(persona_id,enabled,paused,quiet_start_minutes,quiet_end_minutes,daily_wake_budget,wakes_used,budget_day,next_wake_at,next_reason) VALUES(?1,1,1,1380,480,7,3,'2026-10-14','2099-01-01T00:00:00Z','Preserved wake')",[&p.id]).unwrap();
        if existing_file_policy {
            edit(root.path(), &p.id, |doc| {
                doc["heartbeat"]["daily_wake_budget"] = toml_edit::value(12)
            });
        } else {
            edit(root.path(), &p.id, |doc| {
                doc.remove("heartbeat");
            });
        }
        drop(db);
        let db = Database::open(root.path()).unwrap();
        let saved = db.activity_settings(&p.id).unwrap();
        assert_eq!(
            saved.daily_wake_budget,
            if existing_file_policy { 12 } else { 7 }
        );
        assert!(!saved.enabled);
        assert_eq!(saved.next_wake_at.as_deref(), Some("2099-01-01T00:00:00Z"));
        let conn = db.connection.lock().unwrap();
        let runtime: (i64, String) = conn
            .query_row(
                "SELECT wakes_used,budget_day FROM persona_activity_runtime WHERE persona_id=?1",
                [&p.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(runtime, (3, "2026-10-14".into()));
        assert!(
            conn.prepare("SELECT enabled FROM persona_activity_runtime")
                .is_err()
        );
        assert!(
            conn.prepare("SELECT * FROM persona_activity_settings")
                .is_err()
        );
        drop(conn);
        edit(root.path(), &p.id, |doc| {
            doc["heartbeat"]["daily_wake_budget"] = toml_edit::value(8)
        });
        drop(db);
        let db = Database::open(root.path()).unwrap();
        assert_eq!(db.activity_settings(&p.id).unwrap().daily_wake_budget, 8);
    }
}

#[test]
fn invalid_file_during_generation_settles_the_step_and_can_recover() {
    let (root, db, p) = fixture("responses");
    enable(&db, &p);
    let request = wake(&db, &p);
    edit(root.path(), &p.id, |doc| {
        doc["heartbeat"]["daily_wake_budget"] = toml_edit::value(25)
    });
    db.finish_activity_call(&request, "completed", None, Some(&native(&p, quiet(&p))))
        .unwrap();
    let status: String = db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT status FROM activity_calls WHERE id=?1",
            [&request.call_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "cancelled");
    edit(root.path(), &p.id, |doc| {
        doc["heartbeat"]["daily_wake_budget"] = toml_edit::value(24)
    });
    assert_eq!(db.activity_snapshot(&p.id).unwrap().phase, "waiting");
    // Recover scheduling without restarting or leaving a stuck running step.
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    due(&db, &p);
    assert!(db.claim_due_activity(&p.id).unwrap().is_some());
}

#[test]
fn enabling_in_file_during_chat_establishes_plan_state_without_spending_a_wake() {
    let (root, db, p) = fixture("responses");
    let _turn = db.enqueue_turn(&p, "Hello").unwrap();
    edit(root.path(), &p.id, |doc| {
        doc["heartbeat"]["enabled"] = toml_edit::value(true)
    });
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    let snapshot = db.activity_snapshot(&p.id).unwrap();
    assert!(
        snapshot.activity_id.is_some(),
        "user chat must not roll back file-enabled plan state"
    );
    let used: i64 = db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT wakes_used FROM persona_activity_runtime WHERE persona_id=?1",
            [&p.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(used, 0);
}

#[test]
fn newly_created_personas_have_disabled_overnight_defaults_without_changing_existing_files() {
    let root = tempfile::tempdir().unwrap();
    crate::config::initialize(root.path()).unwrap();
    let first = crate::config::load(root.path()).unwrap().personas.remove(0);
    let defaults = policy::read(root.path(), &first.id).unwrap();
    assert_eq!(
        defaults,
        Heartbeat::from_controls(false, Some(1380), Some(480), 4)
    );
    // Existing settings remain explicit and are not rewritten by creation.
    let existing = Heartbeat::from_controls(true, None, None, 9);
    policy::write(root.path(), &first.id, &existing).unwrap();
    let path = policy::path(root.path(), &first.id).unwrap();
    let before = std::fs::read(&path).unwrap();
    let second = crate::personas::create(root.path(), "Second").unwrap();
    assert_eq!(policy::read(root.path(), &second).unwrap(), defaults);
    assert_eq!(std::fs::read(path).unwrap(), before);
    // Old files without a table retain the established disabled/no-quiet fallback.
    edit(root.path(), &first.id, |doc| {
        doc.remove("heartbeat");
    });
    assert_eq!(
        policy::read(root.path(), &first.id).unwrap(),
        Heartbeat::default()
    );
}

#[test]
fn startup_removes_legacy_pause_without_resuming_or_changing_other_settings() {
    for (enabled, paused) in [(true, true), (true, false), (false, true), (false, false)] {
        let (root, db, p) = fixture("responses");
        let path = policy::path(root.path(), &p.id).unwrap();
        edit(root.path(), &p.id, |doc| {
            doc["heartbeat"]["enabled"] = toml_edit::value(enabled);
            doc["heartbeat"]["paused"] = toml_edit::value(paused);
            doc["heartbeat"]["daily_wake_budget"] = toml_edit::value(7);
        });
        let mut expected = crate::settings::document(&path).unwrap();
        expected["heartbeat"]
            .as_table_like_mut()
            .unwrap()
            .remove("paused");
        expected["heartbeat"]["enabled"] = toml_edit::value(enabled && !paused);
        assert_eq!(
            policy::read(root.path(), &p.id).unwrap().enabled,
            enabled && !paused
        );
        drop(db);
        let core = crate::Core::open(root.path()).unwrap();
        assert_eq!(
            core.activity_settings(&p.id).unwrap().enabled,
            enabled && !paused
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            expected.to_string()
        );
        drop(core);
        // Startup normalization is idempotent.
        policy::remove_legacy_pause(root.path(), &p.id).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            expected.to_string()
        );
    }
}

#[test]
fn disabling_preserves_progress_and_intents_until_reenabled_or_explicitly_stopped() {
    let (root, db, p) = fixture("responses");
    enable(&db, &p);
    reply(&db, &p, proposal(vec![create("commitment")]));
    let request = wake(&db, &p);
    db.finish_activity_call(&request, "completed", None, Some(&native(&p, quiet(&p))))
        .unwrap();
    let before = db.activity_snapshot(&p.id).unwrap();
    let saved_intent = intent(&db);
    db.save_activity_settings(&p.id, false, None, None, 24)
        .unwrap();
    due(&db, &p);
    assert!(db.claim_due_activity(&p.id).unwrap().is_none());
    let disabled = db.activity_snapshot(&p.id).unwrap();
    assert!(!disabled.enabled);
    assert!(disabled.next_check_at.is_none());
    assert_eq!(disabled.activity_id, before.activity_id);
    assert_eq!(disabled.summary, before.summary);
    assert_eq!(intent(&db), saved_intent);
    assert!(!crate::settings::document(&policy::path(root.path(), &p.id).unwrap()).unwrap()["heartbeat"]
        .as_table_like().unwrap().contains_key("paused"));
    enable(&db, &p);
    let resumed = db.activity_snapshot(&p.id).unwrap();
    assert_eq!(resumed.activity_id, before.activity_id);
    assert_eq!(resumed.summary, before.summary);
    assert_eq!(intent(&db), saved_intent);
    db.save_activity_settings(&p.id, false, None, None, 24)
        .unwrap();
    db.stop_activity(&p.id).unwrap();
    assert!(db.activity_snapshot(&p.id).unwrap().activity_id.is_none());
    let status: String = db
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT status FROM share_intents WHERE id=?1",
            [saved_intent],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "cancelled");
}
