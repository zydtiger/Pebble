use super::*;
use crate::{Core, ModelRequest, retrieval::Call};
use serde_json::{Value, json};
fn lookup(db: &Database, r: &ModelRequest, name: &str, args: Value) -> Result<Value, Error> {
    db.retrieve_history(
        r,
        &Call {
            id: "lookup".into(),
            name: name.into(),
            arguments: args.to_string(),
        },
    )
}
fn search(content: Value) -> Value {
    json!({"content":content,"date_from":null,"date_to":null,"conversation_id":null,"limit":20,"cursor":null})
}
#[test]
fn history_visibility_is_receipt_based_across_membership_private_and_snapshot_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let core = Core::open(root.path()).unwrap();
    core.create_persona("Other").unwrap();
    let people = core.workspace().unwrap().personas;
    let p = &people[0];
    let other = &people[1];
    let db = &core.database;
    let group = core
        .create_group("Past room", std::slice::from_ref(&other.id))
        .unwrap();
    let pre = db
        .enqueue_room(&group.id, "prejoin secret 中文", &[], &people)
        .unwrap()
        .0;
    core.add_group_member(&group.id, &p.id).unwrap();
    let received = db
        .enqueue_room(&group.id, "visible 中文 O'Brien %_", &[], &people)
        .unwrap()
        .0;
    core.remove_group_member(&group.id, &p.id).unwrap();
    let post = db
        .enqueue_room(&group.id, "postleave secret 中文", &[], &people)
        .unwrap()
        .0;
    let secret = db.enqueue_turn(other, "private other 中文").unwrap();
    let own = db.enqueue_turn(p, "private own 中文").unwrap();
    let own = db.activate_turn(p, &own).unwrap();
    let request = db.request(&own.call_id).unwrap();
    let future = db
        .enqueue_turn(p, "future after frozen snapshot 中文")
        .unwrap();
    let result = lookup(db, &request, "search_history", search(json!("中文"))).unwrap();
    let ids: Vec<_> = result["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["message_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&received.id.as_str()));
    assert!(ids.contains(&own.user.id.as_str()));
    for id in [&pre.id, &post.id, &secret.user.id, &future.user.id] {
        assert!(!ids.contains(&id.as_str()));
        assert!(
            lookup(
                db,
                &request,
                "read_history",
                json!({"message_id":id,"before":4,"after":4})
            )
            .is_err()
        );
    }
    let literal = lookup(db, &request, "search_history", search(json!("O'Brien %_"))).unwrap();
    assert_eq!(literal["messages"].as_array().unwrap().len(), 1);
    let literal = lookup(db, &request, "search_history", search(json!("%"))).unwrap();
    assert_eq!(literal["messages"].as_array().unwrap().len(), 1);
    assert!(lookup(db, &request, "search_history", search(Value::Null)).is_err());
    let room = lookup(
        db,
        &request,
        "read_history",
        json!({"message_id":received.id,"before":4,"after":4}),
    )
    .unwrap();
    assert_eq!(room["messages"].as_array().unwrap().len(), 1);
}
#[test]
fn history_unicode_bounds_pagination_dates_and_native_checkpoint_survive_retry() {
    for api in ["chat_completions", "responses"] {
        let root = tempfile::tempdir().unwrap();
        let core = Core::open(root.path()).unwrap();
        let p = core.workspace().unwrap().personas.remove(0);
        let db = &core.database;
        let first = db
            .enqueue_turn(
                &p,
                &format!("{}中文 needle{}", "前".repeat(1000), "\0".repeat(5000)),
            )
            .unwrap();
        let t = db.activate_turn(&p, &first).unwrap();
        db.finish_turn(&t, "", MessageStatus::Failed, Some("fixture"), None)
            .unwrap();
        let second = db.enqueue_turn(&p, "中文 needle second").unwrap();
        let t = db.activate_turn(&p, &second).unwrap();
        let mut request = db.request(&t.call_id).unwrap();
        request.protocol = api.into();
        let mut args = search(json!("needle"));
        args["limit"] = json!(1);
        let found = lookup(db, &request, "search_history", args.clone()).unwrap();
        assert_eq!(found["messages"][0]["message_id"], first.user.id);
        assert_eq!(found["messages"][0]["truncated"], true);
        assert!(
            found["messages"][0]["text"]
                .as_str()
                .unwrap()
                .contains("needle")
        );
        assert_eq!(found["more"], true);
        args["cursor"] = found["next_cursor"].clone();
        let page = lookup(db, &request, "search_history", args).unwrap();
        assert_eq!(page["messages"][0]["message_id"], second.user.id);
        let mut dates = search(Value::Null);
        dates["date_from"] = json!("2026-02-30");
        assert!(lookup(db, &request, "search_history", dates.clone()).is_err());
        dates["date_from"] = json!("2099-01-01");
        dates["date_to"] = json!("2098-01-01");
        assert!(lookup(db, &request, "search_history", dates.clone()).is_err());
        dates["date_to"] = json!("2099-01-01");
        assert!(
            lookup(db, &request, "search_history", dates).unwrap()["messages"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            lookup(
                db,
                &request,
                "read_history",
                json!({"message_id":first.user.id,"before":5,"after":0})
            )
            .is_err()
        );
        let mut invalid = search(json!("needle"));
        invalid["limit"] = json!(21);
        assert!(lookup(db, &request, "search_history", invalid).is_err());
        let evidence = lookup(
            db,
            &request,
            "read_history",
            json!({"message_id":first.user.id,"before":4,"after":4}),
        )
        .unwrap();
        assert!(evidence.to_string().len() < 32768);
        let output = if api == "responses" {
            json!({"type":"function_call","call_id":"saved","name":"read_history","arguments":json!({"message_id":first.user.id,"before":0,"after":0}).to_string()})
        } else {
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"saved","type":"function","function":{"name":"read_history","arguments":json!({"message_id":first.user.id,"before":0,"after":0}).to_string()}}]})
        };
        let result = crate::memory::result(api, "saved", &evidence.to_string());
        db.reject_delegation_arguments(&request, &[output], &result)
            .unwrap();
        db.finish_turn(&t, "", MessageStatus::Failed, Some("after history"), None)
            .unwrap();
        let retry = db.enqueue_retry(&p, &t.reply.id).unwrap();
        let retry = db.activate_turn(&p, &retry).unwrap();
        let replay = db.request(&retry.call_id).unwrap();
        assert_eq!(replay.history_cutoff, request.history_cutoff);
        assert!(
            replay
                .items
                .iter()
                .any(|i| serde_json::from_slice::<Value>(&i.payload).unwrap() == result)
        );
        assert_eq!(crate::retrieval::used(&replay), 1);
        db.finish_turn(
            &retry,
            "",
            MessageStatus::Failed,
            Some("retry failed"),
            None,
        )
        .unwrap();
        let group = core
            .create_group("Later receipts", std::slice::from_ref(&p.id))
            .unwrap();
        let later = db
            .enqueue_room(
                &group.id,
                "later history needle",
                &[],
                std::slice::from_ref(&p),
            )
            .unwrap()
            .0;
        // A deliberate context change does not widen the original call's history evidence.
        let maintenance = db.begin_context_maintenance(&p, false).unwrap();
        db.publish_maintenance(&maintenance, &p).unwrap();
        let fresh = db.enqueue_retry(&p, &retry.reply.id).unwrap();
        let fresh = db.activate_turn(&p, &fresh).unwrap();
        let frozen = db.request(&fresh.call_id).unwrap();
        assert_eq!(frozen.history_cutoff, request.history_cutoff);
        assert!(
            lookup(
                db,
                &frozen,
                "read_history",
                json!({"message_id":later.id,"before":0,"after":0})
            )
            .is_err()
        );
        assert!(
            frozen
                .items
                .iter()
                .any(|i| String::from_utf8_lossy(&i.payload).contains("needle second"))
        );
    }
}
