mod classifier_support;
mod group_support;
mod live_support;
mod responses_support;
use classifier_support::{Reply, Server, answer, configure, finish, submit};
use group_support::{count, db};
use live_support::isolated;

#[test]
fn oversized_mandatory_trigger_fails_without_http_history_state_or_coverage_consumption() {
    for api in ["chat_completions", "responses"] {
        let (root, core, id) = isolated();
        let server = Server::handling(move |_| Reply::json(answer(api, "{\"respond\":true}")));
        configure(&core, None, &server.url, api, "");
        let mut settings = core.classifier_settings(None).unwrap();
        settings.connection.context_len = Some(3000);
        settings.connection.max_output_tokens = Some(99);
        core.save_classifier_settings(None, &settings, &pebble_core::CredentialChange::Keep)
            .unwrap();
        let group = core
            .create_group("Budget", std::slice::from_ref(&id))
            .unwrap();
        let before = core.response_settings(&id).unwrap();
        let storage = db(root.path());
        let tables = ["model_items", "model_item_events", "model_calls"];
        let counts = tables.map(|table| count(&storage, &format!("SELECT COUNT(*) FROM {table}")));
        let text = "最新 mandatory trigger ".repeat(1000);
        let (post, rx) = submit(&core, &group.id, &text, &[]);
        finish(&post, &rx);
        assert!(server.requests.try_recv().is_err());
        let decisions = core.participation(&group.id).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].status, "failed");
        assert_eq!(decisions[0].respond, None);
        assert!(
            decisions[0].error.contains("mandatory input")
                && decisions[0].error.contains("before HTTP")
        );
        assert_eq!(core.response_settings(&id).unwrap(), before);
        assert_eq!(
            tables.map(|table| count(&storage, &format!("SELECT COUNT(*) FROM {table}"))),
            counts
        );
        assert_eq!(core.room_messages(&group.id).unwrap().len(), 1);
        assert_eq!(
            core.room_messages(&group.id).unwrap()[0].content,
            text.trim()
        );
        assert_eq!(count(&storage, "SELECT COUNT(*) FROM persona_events"), 1);
    }
}

fn input(body: &serde_json::Value, api: &str) -> serde_json::Value {
    let text = if api == "responses" {
        body["input"][1]["content"][0]["text"].as_str().unwrap()
    } else {
        body["messages"][1]["content"].as_str().unwrap()
    };
    serde_json::from_str(text).unwrap()
}

fn large_round(api: &'static str, limit: Option<u64>) -> u64 {
    let (root, core, target) = isolated();
    let mut required = Vec::new();
    for index in 0..10 {
        required.push(core.create_persona(&format!("Member {index}")).unwrap().id);
    }
    let chat = Server::handling(move |_| {
        let text = live_support::structured(&"完整的同轮已接受回复。".repeat(80));
        Reply::stream(if api == "responses" {
            responses_support::candidate(&text)
        } else {
            live_support::candidate(&text)
        })
    });
    live_support::configure(root.path(), &chat.url, "mock", api, "");
    let checked_target = target.clone();
    let classifier = Server::handling(move |body| {
        let data = input(body, api);
        let initial = data["persona"]["id"] == checked_target
            && data["recent_group_transcript"].as_array().unwrap().len() == 1;
        Reply::json(answer(
            api,
            if initial {
                "{\"respond\":true}"
            } else {
                "{\"respond\":false}"
            },
        ))
    });
    configure(&core, None, &classifier.url, api, "");
    let mut settings = core.classifier_settings(None).unwrap();
    settings.connection.context_len = Some(65536);
    settings.connection.max_output_tokens = Some(99);
    core.save_classifier_settings(None, &settings, &pebble_core::CredentialChange::Keep)
        .unwrap();
    if let Some(limit) = limit {
        configure(&core, Some(&target), &classifier.url, api, "");
        let mut settings = core.classifier_settings(Some(&target)).unwrap();
        settings.connection.context_len = Some(limit);
        settings.connection.max_output_tokens = Some(99);
        core.save_classifier_settings(
            Some(&target),
            &settings,
            &pebble_core::CredentialChange::Keep,
        )
        .unwrap();
    }
    let mut members = required.clone();
    members.push(target.clone());
    let room = core
        .create_group("All required predecessors", &members)
        .unwrap();
    let before = core.response_settings(&target).unwrap();
    let storage = db(root.path());
    let counts = || {
        [
        storage.query_row("SELECT COUNT(*) FROM model_calls WHERE persona_id=?1",[&target],|r|r.get::<_,i64>(0)).unwrap(),
        storage.query_row("SELECT COUNT(*) FROM model_items i JOIN model_sessions s ON s.id=i.session_id WHERE s.persona_id=?1",[&target],|r|r.get::<_,i64>(0)).unwrap(),
        storage.query_row("SELECT COUNT(*) FROM model_item_events e JOIN model_items i ON i.id=e.item_id JOIN model_sessions s ON s.id=i.session_id WHERE s.persona_id=?1",[&target],|r|r.get::<_,i64>(0)).unwrap(),
        storage.query_row("SELECT COALESCE(MAX(covered_event_sequence),0) FROM model_sessions WHERE persona_id=?1",[&target],|r|r.get::<_,i64>(0)).unwrap(),
    ]
    };
    let before_counts = counts();
    let (post, rx) = submit(
        &core,
        &room.id,
        "Ten required members reply before the optional target.",
        &required,
    );
    finish(&post, &rx);
    let messages = core.room_messages(&room.id).unwrap();
    let replies: Vec<_> = messages
        .iter()
        .filter(|m| m.sender_kind == "persona")
        .collect();
    assert_eq!(replies.len(), 10);
    assert!(replies.iter().all(|reply| reply.content.len() > 500));
    assert_eq!(core.response_settings(&target).unwrap(), before);
    assert_eq!(counts(), before_counts);
    assert_eq!(
        storage
            .query_row(
                "SELECT COUNT(*) FROM persona_events WHERE persona_id=?1",
                [&target],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        11
    );
    let target_requests: Vec<_> = classifier
        .requests
        .try_iter()
        .map(|(_, body)| body)
        .filter(|body| input(body, api)["persona"]["id"] == target)
        .collect();
    if limit.is_some() {
        assert_eq!(
            target_requests.len(),
            1,
            "Only initial classification may reach HTTP; oversized reassessments must fail before it"
        );
        let decisions = core.participation(&room.id).unwrap();
        assert!(decisions.iter().any(|d| d.persona_id == target
            && d.status == "failed"
            && d.respond.is_none()
            && d.error.contains("mandatory input")
            && d.error.contains("before HTTP")));
        return 0;
    }
    let body = target_requests
        .iter()
        .find(|body| {
            let data = input(body, api);
            data["latest_trigger"]["message_id"] == post.message.id
                && data["recent_group_transcript"].as_array().unwrap().len() == 11
        })
        .expect("Reassessment must include the root and all ten required completed predecessors");
    let data = input(body, api);
    let transcript = data["recent_group_transcript"].as_array().unwrap();
    for reply in replies {
        let event = transcript
            .iter()
            .find(|event| event["message_id"] == reply.id)
            .expect("Every required predecessor must remain present");
        assert_eq!(event["text"], reply.content);
    }
    assert_eq!(transcript[0]["message_id"], post.message.id);
    assert!(
        transcript
            .windows(2)
            .all(|pair| pair[0]["event_sequence"].as_i64() < pair[1]["event_sequence"].as_i64())
    );
    // One byte below the complete mandatory request + output cap must fail.
    serde_json::to_vec(body).unwrap().len() as u64 + 128 + 99 - 1
}

#[test]
fn more_than_eight_required_predecessors_keep_full_text_and_overflow_without_consumption() {
    for api in ["chat_completions", "responses"] {
        let limit = large_round(api, None);
        large_round(api, Some(limit));
    }
}
