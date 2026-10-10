mod live_support;
use live_support::*;
use pebble_core::{
    ChatEventKind, Core, CredentialChange, Error, ResponseAction, ResponseUpdate, SettingsUpdate,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::Path;

fn db(root: &Path) -> Connection {
    Connection::open(root.join("data/app.sqlite")).unwrap()
}
fn update(core: &Core, id: &str, action: ResponseAction) -> ResponseUpdate {
    let expected = core.response_settings(id).unwrap();
    ResponseUpdate {
        schema_json: expected.schema_json.clone(),
        initial_state_json: expected.initial_state_json.clone(),
        expected,
        action,
    }
}
fn schema(update: &mut ResponseUpdate, edit: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_str(&update.schema_json).unwrap();
    edit(&mut value);
    update.schema_json = value.to_string();
}
fn files(root: &Path, id: &str) -> (Vec<u8>, Vec<u8>) {
    let dir = root.join("personas").join(id);
    (
        std::fs::read(dir.join("config.toml")).unwrap(),
        std::fs::read(dir.join("response_schema.json")).unwrap(),
    )
}
fn state(core: &Core, id: &str) -> Value {
    serde_json::from_str(&core.response_settings(id).unwrap().current_state_json).unwrap()
}
fn accepted(core: &Core, id: &str) -> std::sync::Arc<pebble_core::ChatTask> {
    let (task, rx) = start(core, id, "synthetic user");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    task
}

#[test]
fn defaults_only_keep_and_reset_create_explicit_boundaries_preserving_native_history_and_restart() {
    let server = Server::new(vec![
        stream(candidate(
            &json!({"reply":"first", "state":{"affinity_user":21,"mood":"curious"}}).to_string(),
        )),
        stream(candidate(
            &json!({"reply":"after reset", "state":{"feeling":"warm"}}).to_string(),
        )),
    ]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let first = accepted(&core, &id);
    let original = core.model_request(&first.id).unwrap();
    let messages = core.messages(&id).unwrap();
    let mut defaults = update(&core, &id, ResponseAction::InitialDefaultsOnly);
    defaults.initial_state_json = json!({"affinity_user":9,"mood":"quiet"}).to_string();
    core.save_response_settings(&id, &defaults).unwrap();
    let saved = core.response_settings(&id).unwrap();
    assert_eq!(saved.session_id, defaults.expected.session_id);
    assert_eq!(
        state(&core, &id),
        json!({"affinity_user":21,"mood":"curious"})
    );
    assert_eq!(
        serde_json::from_str::<Value>(&saved.initial_state_json).unwrap()["affinity_user"],
        9
    );
    let mut keep = update(&core, &id, ResponseAction::FreshContextKeepState);
    schema(&mut keep, |s| {
        s["properties"]["state"]["properties"]["mood"]["description"] =
            json!("Current feeling, with no automatic change.")
    });
    core.save_response_settings(&id, &keep).unwrap();
    let kept = core.response_settings(&id).unwrap();
    assert_ne!(kept.session_id, keep.expected.session_id);
    assert_eq!(kept.current_state_json, keep.expected.current_state_json);
    assert_eq!(kept.active_schema_json, Some(kept.schema_json.clone()));
    let mut reset = update(&core, &id, ResponseAction::FreshContextResetState);
    schema(
        &mut reset,
        |s| s["properties"]["state"] = json!({"type":"object","additionalProperties":false,"properties":{"feeling":{"type":"string","minLength":1}},"required":["feeling"]}),
    );
    reset.initial_state_json = json!({"feeling":"quiet"}).to_string();
    core.save_response_settings(&id, &reset).unwrap();
    assert_eq!(state(&core, &id), json!({"feeling":"quiet"}));
    assert_ne!(
        core.response_settings(&id).unwrap().session_id,
        kept.session_id
    );
    assert_eq!(
        core.messages(&id)
            .unwrap()
            .iter()
            .map(|m| &m.id)
            .collect::<Vec<_>>(),
        messages.iter().map(|m| &m.id).collect::<Vec<_>>()
    );
    let second = accepted(&core, &id);
    let after = core.model_request(&second.id).unwrap();
    assert_eq!(after.state_before, Some(json!({"feeling":"quiet"})));
    let user: Value = serde_json::from_slice(&after.items.last().unwrap().payload).unwrap();
    assert!(
        !user["content"]
            .as_str()
            .unwrap()
            .contains(&first.messages[0].id)
    );
    let historical = core.model_request(&first.id).unwrap();
    assert_eq!(historical.response_schema, original.response_schema);
    assert_eq!(historical.state_before, original.state_before);
    assert_eq!(
        historical
            .items
            .iter()
            .map(|i| (&i.id, &i.payload))
            .collect::<Vec<_>>(),
        original
            .items
            .iter()
            .map(|i| (&i.id, &i.payload))
            .collect::<Vec<_>>()
    );
    core.fresh_context(&id).unwrap();
    assert_eq!(state(&core, &id), json!({"feeling":"warm"}));
    let settings = core.settings(Some(&id)).unwrap();
    core.save_settings(
        Some(&id),
        &SettingsUpdate {
            chat: settings.chat,
            credential: CredentialChange::Keep,
            context_window_tokens: settings.context_window_tokens,
            max_output_tokens: settings.max_output_tokens,
            persona: settings.persona,
        },
    )
    .unwrap();
    let snapshot = core.response_settings(&id).unwrap();
    drop(first);
    drop(second);
    drop(core);
    let reopened = reopen(root.path());
    assert_eq!(reopened.response_settings(&id).unwrap(), snapshot);
    assert_eq!(
        reopened
            .model_request(&original.call_id)
            .unwrap()
            .response_schema,
        original.response_schema
    );
}

#[test]
fn invalid_candidates_and_incompatible_keep_fail_before_any_file_state_or_session_change() {
    let (root, core, id) = isolated();
    core.fresh_context(&id).unwrap();
    let before = core.response_settings(&id).unwrap();
    let original_files = files(root.path(), &id);
    for case in 0..6 {
        let mut change = update(&core, &id, ResponseAction::FreshContextKeepState);
        match case {
            0 => change.schema_json = "broken JSON".into(),
            1 => schema(&mut change, |s| s["additionalProperties"] = json!(true)),
            2 => {
                change.initial_state_json = json!({"affinity_user":200,"mood":"quiet"}).to_string()
            }
            3 => {
                schema(&mut change, |s| {
                    s["properties"]["state"]["properties"]["affinity_user"]["maximum"] = json!(10)
                });
                change.initial_state_json = json!({"affinity_user":9,"mood":"quiet"}).to_string();
            }
            4 => {
                change.action = ResponseAction::InitialDefaultsOnly;
                schema(&mut change, |s| {
                    s["description"] = json!("changed semantics")
                });
            }
            _ => change.initial_state_json = "broken JSON".into(),
        }
        assert!(
            core.save_response_settings(&id, &change).is_err(),
            "case {case}"
        );
        assert_eq!(files(root.path(), &id), original_files, "case {case}");
        assert_eq!(core.response_settings(&id).unwrap(), before, "case {case}");
    }
    // JSON Schema can permit null; the editable initial defaults still need TOML representation.
    let mut null = update(&core, &id, ResponseAction::FreshContextResetState);
    schema(
        &mut null,
        |s| s["properties"]["state"] = json!({"type":"object","additionalProperties":false,"properties":{"optional":{"type":"null"}},"required":["optional"]}),
    );
    null.initial_state_json = json!({"optional":null}).to_string();
    assert!(
        core.save_response_settings(&id, &null)
            .unwrap_err()
            .to_string()
            .contains("TOML")
    );
    assert_eq!(files(root.path(), &id), original_files);
    assert_eq!(core.response_settings(&id).unwrap(), before);
}

#[test]
fn busy_changes_are_persona_scoped_and_stale_state_context_or_defaults_require_reload() {
    let server = Server::new(vec![Plan {
        status: 200,
        body: candidate(
            &json!({"reply":"accepted","state":{"affinity_user":22,"mood":"happy"}}).to_string(),
        ),
        delay_headers: true,
        delay_tail: false,
    }]);
    let (root, core, id) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let other = core.create_persona("Kai").unwrap();
    let stale = update(&core, &id, ResponseAction::FreshContextResetState);
    let (task, rx) = start(&core, &id, "busy");
    let (queued, queued_rx) = start(&core, &id, "pending input");
    assert!(matches!(
        core.save_response_settings(&id, &stale),
        Err(Error::Busy)
    ));
    core.save_response_settings(
        &other.id,
        &update(&core, &other.id, ResponseAction::FreshContextResetState),
    )
    .unwrap();
    queued.cancel();
    assert_eq!(
        terminal(&queued_rx).last().unwrap().kind,
        ChatEventKind::Cancelled
    );
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    assert!(
        core.save_response_settings(&id, &stale)
            .unwrap_err()
            .to_string()
            .contains("Reload")
    );
    let stale = update(&core, &id, ResponseAction::FreshContextKeepState);
    core.fresh_context(&id).unwrap();
    assert!(
        core.save_response_settings(&id, &stale)
            .unwrap_err()
            .to_string()
            .contains("Reload")
    );
    let stale = update(&core, &id, ResponseAction::FreshContextKeepState);
    let mut defaults = update(&core, &id, ResponseAction::InitialDefaultsOnly);
    defaults.initial_state_json = json!({"affinity_user":10,"mood":"quiet"}).to_string();
    core.save_response_settings(&id, &defaults).unwrap();
    assert!(
        core.save_response_settings(&id, &stale)
            .unwrap_err()
            .to_string()
            .contains("Reload")
    );
    assert_eq!(state(&core, &id)["affinity_user"], 22);
    drop(task);
}

#[test]
fn late_sql_commit_failure_preserves_state_and_active_contract_with_explicit_reload_recovery() {
    let (root, core, id) = isolated();
    core.fresh_context(&id).unwrap();
    let before = core.response_settings(&id).unwrap();
    let connection = db(root.path());
    connection.execute_batch("CREATE TABLE contract_commit_probe(persona_id TEXT REFERENCES personas(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fail_contract_commit AFTER INSERT ON model_sessions BEGIN INSERT INTO contract_commit_probe VALUES ('missing'); END;").unwrap();
    let mut change = update(&core, &id, ResponseAction::FreshContextResetState);
    schema(&mut change, |s| {
        s["description"] = json!("New explicitly selected contract")
    });
    change.initial_state_json = json!({"affinity_user":7,"mood":"quiet"}).to_string();
    let error = core
        .save_response_settings(&id, &change)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("files were published") && error.contains("Reload"),
        "{error}"
    );
    let after = core.response_settings(&id).unwrap();
    assert_ne!(after.schema_json, before.schema_json);
    assert_ne!(after.initial_state_json, before.initial_state_json);
    assert_eq!(after.current_state_json, before.current_state_json);
    assert_eq!(after.session_id, before.session_id);
    assert_eq!(after.active_schema_json, before.active_schema_json);
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM model_sessions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(
        core.save_response_settings(&id, &change)
            .unwrap_err()
            .to_string()
            .contains("Reload")
    );
    connection
        .execute_batch("DROP TRIGGER fail_contract_commit;")
        .unwrap();
    // Reloaded published defaults still need an explicit state/context action.
    core.save_response_settings(
        &id,
        &update(&core, &id, ResponseAction::FreshContextResetState),
    )
    .unwrap();
    assert_eq!(state(&core, &id)["affinity_user"], 7);
    assert_ne!(
        core.response_settings(&id).unwrap().session_id,
        before.session_id
    );
}
