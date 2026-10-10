mod classifier_support;
mod live_support;
use classifier_support::*;
use live_support::{isolated, start, terminal};
use pebble_core::{ChatEventKind, CredentialChange, SettingsUpdate};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[test]
#[ignore = "explicit selected Qwen and chat services; synthetic roots only"]
fn opt_in_selected_participation_regression_and_group_latency_under_chat_load() {
    let report_path = std::env::var("PEBBLE_CLASSIFIER_REPORT")
        .ok()
        .map(std::path::PathBuf::from);
    if let Some(path) = &report_path {
        assert!(
            path.is_absolute() && path.parent().is_some_and(std::path::Path::is_dir),
            "PEBBLE_CLASSIFIER_REPORT requires an absolute path in an existing directory"
        );
    }
    let url = std::env::var("PEBBLE_CLASSIFIER_API_ROOT").expect("set PEBBLE_CLASSIFIER_API_ROOT");
    let model = std::env::var("PEBBLE_CLASSIFIER_MODEL").expect("set PEBBLE_CLASSIFIER_MODEL");
    let chat_url = std::env::var("PEBBLE_LIVE_API_ROOT").expect("set PEBBLE_LIVE_API_ROOT");
    let chat_model = std::env::var("PEBBLE_LIVE_MODEL").expect("set PEBBLE_LIVE_MODEL");
    let raw = Arc::new(Mutex::new(Vec::new()));
    let captured = raw.clone();
    let proxy = Server::handling(move |body| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let response: Value = runtime.block_on(async {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap()
                .post(format!("{}/chat/completions", url.trim_end_matches('/')))
                .json(body)
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap()
        });
        assert!(
            response["choices"][0]["message"]
                .get("reasoning_content")
                .is_none_or(|v| v.is_null() || v.as_str() == Some("")),
            "Selected non-thinking request produced reasoning"
        );
        assert!(
            response["usage"]["completion_tokens_details"]
                .get("reasoning_tokens")
                .is_none_or(|v| v == 0)
        );
        captured
            .lock()
            .unwrap()
            .push(json!({"request":body,"response":response}));
        Reply::json(response)
    });
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/participation-regressions.json")).unwrap();
    let mut results = Vec::new();
    let (mut tp, mut fn_count, mut fp, mut tn) = (0, 0, 0, 0);
    for case in cases {
        let (root, core, a) = isolated();
        let soul = case["soul"].as_str().unwrap();
        let name = soul.split([':', '：']).next().unwrap();
        let mut settings = core.settings(Some(&a)).unwrap();
        settings.persona.as_mut().unwrap().display_name = name.into();
        settings.persona.as_mut().unwrap().soul = soul.into();
        settings.persona.as_mut().unwrap().memory = case["memory"].as_str().unwrap().into();
        core.save_settings(
            Some(&a),
            &SettingsUpdate {
                chat: settings.chat,
                credential: CredentialChange::Keep,
                context_window_tokens: settings.context_window_tokens,
                max_output_tokens: settings.max_output_tokens,
                persona: settings.persona,
            },
        )
        .unwrap();
        let group = core
            .create_group("Frozen synthetic regression", std::slice::from_ref(&a))
            .unwrap();
        let transcript = case["transcript"].as_array().unwrap();
        for text in &transcript[..transcript.len() - 1] {
            let (post, _) = submit(&core, &group.id, text.as_str().unwrap(), &[]);
            assert!(post.group.is_none());
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
        let started = Instant::now();
        let (post, rx) = submit(
            &core,
            &group.id,
            transcript.last().unwrap().as_str().unwrap(),
            &[],
        );
        finish(&post, &rx);
        let d = core
            .participation(&group.id)
            .unwrap()
            .into_iter()
            .find(|d| d.message_id == post.message.id)
            .unwrap();
        assert!(
            matches!(d.status.as_str(), "selected" | "listening"),
            "{}: {}",
            case["id"],
            d.error
        );
        let expected = case["expected"].as_bool().unwrap();
        let actual = d.respond.unwrap();
        match (expected, actual) {
            (true, true) => tp += 1,
            (true, false) => fn_count += 1,
            (false, true) => fp += 1,
            (false, false) => tn += 1,
        };
        results.push(json!({"id":case["id"],"expected":expected,"actual":actual,"decision_ms":d.elapsed_ms,"including_demo_reply_ms":started.elapsed().as_millis()}));
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
    settings.chat.api = "chat_completions".into();
    settings.chat.model = chat_model;
    settings.chat.base_url = chat_url;
    core.save_settings(
        None,
        &SettingsUpdate {
            chat: settings.chat,
            credential: CredentialChange::Clear,
            context_window_tokens: Some(65536),
            max_output_tokens: Some(1024),
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
    let started = Instant::now();
    let (post, rx) = submit(
        &core,
        &group.id,
        "Everyone, I had a rough day and would welcome one short, friendly idea for a calming walk. Anyone who has an idea may join.",
        &[],
    );
    let (mut expected, mut done, mut settled) = (0, 0, false);
    let mut decision_ms = 0;
    while !settled || done < expected {
        let event = rx.recv_timeout(Duration::from_secs(180)).unwrap();
        match event.kind {
            ChatEventKind::Admitted => expected += 1,
            ChatEventKind::GroupSettled => {
                settled = true;
                decision_ms = started.elapsed().as_millis();
            }
            ChatEventKind::Completed => done += 1,
            ChatEventKind::Failed | ChatEventKind::Cancelled => panic!(
                "Synthetic group generation did not complete: {}",
                event.text
            ),
            _ => {}
        }
    }
    for events in [terminal(&r1), terminal(&r2)] {
        assert_eq!(
            events.last().unwrap().kind,
            ChatEventKind::Completed,
            "{:?}",
            events.last().unwrap()
        );
    }
    let decisions = core.participation(&group.id).unwrap();
    assert_eq!(decisions.len(), 4);
    assert!(
        decisions
            .iter()
            .all(|d| matches!(d.status.as_str(), "selected" | "listening"))
    );
    assert_eq!(raw.lock().unwrap().len(), 12);
    let report = json!({"scope":"Integration regression with unchanged frozen labels; bounded persona/attributed room inputs differ from original probe. Not a generalization estimate.","TP":tp,"FN":fn_count,"FP":fp,"TN":tn,"cases":results,"group_members":4,"concurrent_private_chat_calls":2,"complete_group_decision_ms":decision_ms,"group_calls":expected,"decisions":decisions.iter().map(|d|json!({"status":d.status,"elapsed_ms":d.elapsed_ms})).collect::<Vec<_>>(),"raw_requests_and_responses":*raw.lock().unwrap()});
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = report_path {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    drop((t1, t2, post, core, root));
}
