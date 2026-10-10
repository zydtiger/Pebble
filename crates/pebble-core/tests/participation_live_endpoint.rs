mod classifier_support;
mod live_support;
mod participation_live_support;
use classifier_support::{configure, submit};
use live_support::{isolated, start, terminal};
use participation_live_support::*;
use pebble_core::{ChatEventKind, CredentialChange, SettingsUpdate};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

fn check_runtime_decisions(report: &Value) -> Result<(), String> {
    let cases = report["cases"].as_array().unwrap();
    let scopes = cases
        .iter()
        .map(|case| (case["id"].as_str().unwrap(), &case["final_decisions"]))
        .chain(std::iter::once(("load round", &report["final_decisions"])));
    for (scope, decisions) in scopes {
        for decision in decisions.as_array().unwrap() {
            if decision["status"] == "failed"
                || decision["error"].as_str().is_some_and(|e| !e.is_empty())
            {
                return Err(format!("Runtime classifier failure in {scope}: {decision}"));
            }
        }
    }
    Ok(())
}

#[test]
fn runtime_report_gate_rejects_schema_valid_wrong_role_from_actual_adapter() {
    let mut response = classifier_support::answer("chat_completions", r#"{"respond":true}"#);
    response["choices"][0]["message"]["role"] = json!("user");
    let raw =
        json!({"phase":"primary","http_status":200,"transport_error":null,"response":response});
    assert!(
        valid_classifier(&raw),
        "Raw schema checks alone accept this"
    );
    assert_eq!(boolean(&raw), Some(true));
    let server = classifier_support::Server::new(vec![classifier_support::Reply::json(
        raw["response"].clone(),
    )]);
    let (root, core, a) = isolated();
    configure(&core, None, &server.url, "chat_completions", "");
    let group = core.create_group("Runtime report gate", &[a]).unwrap();
    let (post, rx) = submit(&core, &group.id, "Synthetic participation check", &[]);
    let events = settle(&post, &rx);
    assert!(post.tasks.is_empty());
    assert!(events.iter().all(|event| !matches!(
        event["kind"].as_str().unwrap(),
        "Admitted" | "Failed" | "Cancelled"
    )));
    assert_eq!(core.room_messages(&group.id).unwrap().len(), 1);
    let decisions = core.participation(&group.id).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].status, "failed");
    assert!(!decisions[0].error.is_empty());
    let failed =
        json!({"status":decisions[0].status,"phase":decisions[0].phase,"error":decisions[0].error});
    let path = root.path().join("runtime-report.json");
    for scope in ["frozen case", "load round"] {
        let mut report =
            json!({"cases":[{"id":"wrong-role","final_decisions":[]}],"final_decisions":[]});
        if scope == "frozen case" {
            report["cases"][0]["final_decisions"] = json!([failed]);
        } else {
            report["final_decisions"] = json!([failed]);
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let error = check_runtime_decisions(&saved).unwrap_err();
        println!("Rejected {scope} after publishing report: {error}");
    }
    // Explicitly supplied evidence can also be checked offline without opting into live calls.
    if let Ok(path) = std::env::var("PEBBLE_CLASSIFIER_REPORT") {
        let report: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        check_runtime_decisions(&report).expect("Runtime classifier errors preserved in report");
        println!("Retained runtime decisions accepted offline: {path}");
    }
}

#[test]
#[ignore = "explicit selected Qwen and chat services; synthetic roots only"]
fn opt_in_selected_participation_regression_and_group_latency_under_chat_load() {
    let report_path = std::path::PathBuf::from(
        std::env::var("PEBBLE_CLASSIFIER_REPORT").expect("set absolute PEBBLE_CLASSIFIER_REPORT"),
    );
    assert!(
        report_path.is_absolute() && report_path.parent().is_some_and(std::path::Path::is_dir),
        "Report needs an absolute path in an existing directory before any requests"
    );
    let classifier_url =
        std::env::var("PEBBLE_CLASSIFIER_API_ROOT").expect("set PEBBLE_CLASSIFIER_API_ROOT");
    let model = std::env::var("PEBBLE_CLASSIFIER_MODEL").expect("set PEBBLE_CLASSIFIER_MODEL");
    let chat_url = std::env::var("PEBBLE_LIVE_API_ROOT").expect("set PEBBLE_LIVE_API_ROOT");
    let chat_model = std::env::var("PEBBLE_LIVE_MODEL").expect("set PEBBLE_LIVE_MODEL");
    let clock = Instant::now();
    let raw = Arc::new(Mutex::new(Vec::new()));
    let proxy = relay(classifier_url, "classifier", raw.clone(), clock);
    let chat_proxy = relay(chat_url, "chat", raw.clone(), clock);
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/participation-regressions.json")).unwrap();
    let mut results = Vec::new();
    let (mut tp, mut fn_count, mut fp, mut tn, mut invalid) = (0, 0, 0, 0, 0);
    for case in cases {
        let (root, core, a) = isolated();
        let soul = case["soul"].as_str().unwrap();
        let name = soul.split([':', '：']).next().unwrap();
        let mut settings = core.settings(Some(&a)).unwrap();
        let persona = settings.persona.as_mut().unwrap();
        persona.display_name = name.into();
        persona.soul = soul.into();
        persona.memory = case["memory"].as_str().unwrap().into();
        core.save_settings(
            Some(&a),
            &SettingsUpdate {
                chat: settings.chat,
                credential: CredentialChange::Keep,
                persona: settings.persona,
            },
        )
        .unwrap();
        let group = core
            .create_group("Frozen synthetic regression", std::slice::from_ref(&a))
            .unwrap();
        let transcript = case["transcript"].as_array().unwrap();
        for text in &transcript[..transcript.len() - 1] {
            let (post, rx) = submit(&core, &group.id, text.as_str().unwrap(), &[]);
            settle(&post, &rx);
        }
        configure(
            &core,
            None,
            &proxy.url,
            "chat_completions",
            "qwen_non_thinking",
        );
        let mut connection = core.classifier_settings(None).unwrap();
        connection.connection.model = model.clone();
        core.save_classifier_settings(None, &connection, &CredentialChange::Clear)
            .unwrap();
        let offset = raw.lock().unwrap().len();
        let started = clock.elapsed().as_millis();
        let (post, rx) = submit(
            &core,
            &group.id,
            transcript.last().unwrap().as_str().unwrap(),
            &[],
        );
        let events = settle(&post, &rx);
        let total_ms = clock.elapsed().as_millis() - started;
        let records = raw.lock().unwrap()[offset..].to_vec();
        let initial = primary(&records, &a);
        let actual = initial.as_ref().and_then(boolean);
        let expected = case["expected"].as_bool().unwrap();
        match (expected, actual) {
            (true, Some(true)) => tp += 1,
            (true, Some(false)) => fn_count += 1,
            (false, Some(true)) => fp += 1,
            (false, Some(false)) => tn += 1,
            (_, None) => invalid += 1,
        };
        results.push(json!({"id":case["id"],"expected":expected,"initial_primary_actual":actual,"initial_primary_response":initial,"initial_primary_latency_ms":initial.as_ref().map(|r|r["finished_ms"].as_u64().unwrap() as u128-started),"total_round_including_demo_ms":total_ms,"events":events,"total_classifier_requests":records.len(),"final_decisions":core.participation(&group.id).unwrap().iter().map(|d|json!({"status":d.status,"phase":d.phase,"respond":d.respond,"error":d.error})).collect::<Vec<_>>() }));
        drop((post, core, root));
    }
    let (root, core, a) = isolated();
    let mut members = vec![a.clone()];
    for n in 1..4 {
        members.push(
            core.create_persona(&format!("Synthetic friend {n}"))
                .unwrap()
                .id,
        );
    }
    for persona in core.workspace().unwrap().personas {
        std::fs::write(std::path::Path::new(&persona.directory).join("SOUL.md"),format!("{}: a concise, supportive friend who enjoys sharing simple walking ideas. Reply briefly, in one sentence.",persona.display_name)).unwrap();
    }
    let mut settings = core.settings(None).unwrap();
    settings.chat.enabled = true;
    settings.chat.context_len = Some(65536);
    settings.chat.max_output_tokens = Some(1024);
    settings.chat.api = "chat_completions".into();
    settings.chat.model = chat_model;
    settings.chat.base_url = chat_proxy.url.clone();
    core.save_settings(
        None,
        &SettingsUpdate {
            chat: settings.chat,
            credential: CredentialChange::Clear,
            persona: None,
        },
    )
    .unwrap();
    configure(
        &core,
        None,
        &proxy.url,
        "chat_completions",
        "qwen_non_thinking",
    );
    let mut connection = core.classifier_settings(None).unwrap();
    connection.connection.model = model;
    core.save_classifier_settings(None, &connection, &CredentialChange::Clear)
        .unwrap();
    let (t1, r1) = start(
        &core,
        &a,
        "Synthetic load check: offer one brief walking suggestion.",
    );
    let (t2, r2) = start(
        &core,
        &members[1],
        "Synthetic independent load check: offer one brief walking suggestion.",
    );
    let group = core
        .create_group("Synthetic latency under chat load", &members)
        .unwrap();
    let offset = raw.lock().unwrap().len();
    let started = clock.elapsed().as_millis();
    let (post, rx) = submit(
        &core,
        &group.id,
        "Everyone, I had a rough day and would welcome one short, friendly idea for a calming walk. Anyone who has an idea may join.",
        &[],
    );
    let events = settle(&post, &rx);
    let total_ms = clock.elapsed().as_millis() - started;
    let private_events = [terminal(&r1), terminal(&r2)];
    let records = raw.lock().unwrap()[offset..].to_vec();
    let initial = members
        .iter()
        .filter_map(|id| primary(&records, id))
        .collect::<Vec<_>>();
    let initial_ms = initial
        .iter()
        .filter_map(|r| r["finished_ms"].as_u64())
        .max()
        .map(|end| end as u128 - started);
    let decisions = core.participation(&group.id).unwrap();
    let calls = events.iter().filter(|e| e["kind"] == "Admitted").count();
    let errors = events
        .iter()
        .filter(|e| e["kind"] == "Failed" || e["kind"] == "Cancelled")
        .cloned()
        .collect::<Vec<_>>();
    let captures = raw.lock().unwrap().clone();
    let schema_valid = captures
        .iter()
        .filter(|r| r["service"] == "classifier")
        .all(valid_classifier);
    let nonthinking = captures
        .iter()
        .filter(|r| r["service"] == "classifier")
        .all(nonthinking);
    let report = json!({"scope":"Integration regression using unchanged eight frozen labels and compact attributed runtime inputs, not an independent accuracy estimate or latency guarantee. First primary response per persona is preserved separately from later reassessment/followup/fallback. Capture relay buffers full SSE solely for synthetic raw evidence.","chat_api":"chat_completions","classifier_api":"chat_completions","TP":tp,"FN":fn_count,"FP":fp,"TN":tn,"invalid_initial":invalid,"cases":results,"group_members":members.len(),"concurrent_private_chat_calls":2,"initial_primary_responses_received_ms":initial_ms,"initial_primary_responses":initial,"total_round_settled_ms":total_ms,"group_calls":calls,"group_call_bound":members.len()+2,"group_events":events,"group_errors":errors,"final_decisions":decisions.iter().map(|d|json!({"status":d.status,"phase":d.phase,"respond":d.respond,"slot_status":d.slot_status,"elapsed_ms":d.elapsed_ms,"error":d.error})).collect::<Vec<_>>(),"private_calls_completed":private_events.iter().all(|events|events.last().unwrap().kind==ChatEventKind::Completed),"all_classifier_responses_schema_valid":schema_valid,"all_classifier_responses_nonthinking":nonthinking,"total_classifier_requests":captures.iter().filter(|r|r["service"]=="classifier").count(),"total_chat_requests_including_format_repair":captures.iter().filter(|r|r["service"]=="chat").count(),"raw_requests_and_responses":captures});
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!(
        "TP {tp} / FN {fn_count} / FP {fp} / TN {tn}; invalid {invalid}; initial {initial_ms:?} ms; total {total_ms} ms; calls {calls}/{}; report {}",
        members.len() + 2,
        report_path.display()
    );
    check_runtime_decisions(&report).expect("Runtime classifier errors preserved in report");
    assert_eq!(invalid, 0, "Raw errors preserved in report");
    assert!(schema_valid, "Raw classifier errors preserved in report");
    assert!(
        nonthinking,
        "Raw reasoning capability failure preserved in report"
    );
    assert_eq!(initial.len(), 4);
    assert!(calls <= members.len() + 2);
    assert!(errors.is_empty(), "{errors:?}");
    assert!(report["private_calls_completed"].as_bool().unwrap());
    drop((t1, t2, post, core, root));
}
