use super::*;

#[test]
fn incorrect_acknowledgments_cannot_commit_state_or_consume_task_evidence() {
    let (_root, db, p) = fixture("responses");
    enable(&db, &p);
    let request = wake(&db, &p);
    let call = crate::codex::tool::Call {
        id: "handover-tool".into(),
        arguments: crate::codex::tool::Arguments {
            title: "Test research".into(),
            instructions: "Read one synthetic source".into(),
            request_key: "handover".into(),
            task_id: None,
        },
    };
    let (_, id) = db
        .prepare_activity_delegation(&request, &call, &[], &crate::CodexSettings::default())
        .unwrap();
    let id = id.unwrap();
    db.finish_activity_call(&request, "cancelled", None, None)
        .unwrap();
    db.update_task(&id, |task| {
        task.state = DelegationState::Failed;
        task.detail = "Source unavailable; no finding established.".into();
        Ok(())
    })
    .unwrap();
    db.queue_activity_task_outcome(&id).unwrap();
    let review = wake(&db, &p);
    let valid = reviewed(&p, &review);
    let pair = valid["activity"]["observed_outcomes"][0].clone();
    for acknowledgment in [
        json!([]),
        json!([{"task_id":"wrong","outcome_revision":pair["outcome_revision"]}]),
        json!([{"task_id":id,"outcome_revision":999}]),
        json!([pair.clone(), pair]),
    ] {
        let mut output = valid.clone();
        output["activity"]["observed_outcomes"] = acknowledgment;
        output["state"]["affinity_user"] = json!(77);
        let duplicate = output["activity"]["observed_outcomes"]
            .as_array()
            .unwrap()
            .len()
            > 1;
        let native = native(&p, output);
        assert!(
            db.private_activity_feedback(&review, &native)
                .unwrap()
                .is_some()
        );
        let result = db.finish_activity_call(&review, "completed", None, Some(&native));
        if duplicate {
            result.unwrap(); // Schema rejection settles the call without effects.
        } else {
            assert!(matches!(result, Err(Error::Generation(_))));
        }
        assert_eq!(db.task(&id).unwrap().report_state, "private_queued");
        let current: String = db
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT current_state FROM persona_states WHERE persona_id=?1",
                [&p.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&current).unwrap(),
            p.initial_state
        );
    }
    // The duplicate-list schema failure settles the call. A later wake can still
    // observe the saved failure, without ever claiming successful research.
    let review = wake(&db, &p);
    let mut output = reviewed(&p, &review);
    output["activity"]["summary"] = json!("The source failed; no finding established.");
    db.finish_activity_call(&review, "completed", None, Some(&native(&p, output)))
        .unwrap();
    assert_eq!(db.task(&id).unwrap().report_state, "private_assimilated");
    assert_eq!(db.task(&id).unwrap().state, DelegationState::Failed);
    assert_eq!(count(&db, "delegated_tasks"), 1);
    assert_eq!(count(&db, "messages"), 0);
}

#[test]
fn invalid_fulfillment_rolls_back_whole_plan_but_preserves_chat_and_runtime_feedback() {
    for api in ["chat_completions", "responses"] {
        let (_root, db, p) = fixture(api);
        enable(&db, &p);
        let mut op = create("optional");
        op["condition"] = json!("activity_complete");
        reply(&db, &p, proposal(vec![op]));
        let saved = intent(&db);
        let before = db.activity_snapshot(&p.id).unwrap();
        let mut op = create("optional");
        op["action"] = json!("fulfill");
        op["id"] = json!(saved);
        let mut metadata = proposal(vec![create("optional"), op]);
        metadata["objective"] = json!("Unaccepted objective");
        let turn = db.enqueue_turn(&p, "hi").unwrap();
        let turn = db.activate_turn(&p, &turn).unwrap();
        let output = native(
            &p,
            json!({"reply":"Hello!","state":p.initial_state,"activity":metadata}),
        );
        let message = db
            .finish_turn(
                &turn,
                "Hello!",
                MessageStatus::Complete,
                None,
                Some(&output),
            )
            .unwrap();
        assert_eq!(message.content, "Hello!");
        assert_eq!(message.status, MessageStatus::Complete);
        assert_eq!(intent(&db), saved);
        assert_eq!(count(&db, "share_intents"), 1);
        assert_eq!(
            db.activity_snapshot(&p.id).unwrap().objective,
            before.objective
        );
        let next = db.enqueue_turn(&p, "Continue").unwrap();
        let next = db.activate_turn(&p, &next).unwrap();
        let request = db.request(&next.call_id).unwrap();
        // Accepted native metadata is retained as authored; runtime feedback
        // records its rejection rather than pretending it changed durable state.
        assert!(
            request
                .items
                .iter()
                .any(|i| String::from_utf8_lossy(&i.payload).contains("activity_feedback"))
        );
    }
}
