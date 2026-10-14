use super::{memory_test_support::*, *};
use serde_json::{Value, json};
pub(super) fn tool(api: &str, id: &str, name: &str, args: Value) -> String {
    if api == "responses" {
        super::memory_responses_support::completed(&[
            json!({"id":format!("item-{id}"),"type":"function_call","status":"completed","call_id":id,"name":name,"arguments":args.to_string()}),
        ])
    } else {
        format!(
            "{}{}data: [DONE]\n\n",
            event(
                json!({"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}),
                Value::Null
            ),
            event(json!({}), json!("tool_calls"))
        )
    }
}
#[test]
fn native_history_search_read_memory_final_is_balanced_bounded_and_replays_both_protocols() {
    for api in ["chat_completions", "responses"] {
        let (root, core, id) = isolated();
        let p = core.workspace().unwrap().personas.remove(0);
        let seed = core.database.enqueue_turn(&p, "历史 seed 中文").unwrap();
        core.database
            .finish_pending(&seed, MessageStatus::Cancelled, None)
            .unwrap();
        let finish = || {
            if api == "responses" {
                super::memory_responses_support::success("remembered")
            } else {
                success("remembered")
            }
        };
        let args = json!({"content":"中文","date_from":null,"date_to":null,"conversation_id":null,"limit":5,"cursor":null});
        let server = Server::new(vec![
            stream(tool(api, "search", "search_history", args)),
            stream(tool(
                api,
                "read",
                "read_history",
                json!({"message_id":seed.user.id,"before":0,"after":0}),
            )),
            stream(tool(
                api,
                "save",
                "update_memory",
                json!({"content":"remembered bounded 中文"}),
            )),
            stream(finish()),
            stream(finish()),
        ]);
        configure(root.path(), &server.url, "mock", api, "");
        let (task, rx) = start(&core, &id, "Recall our history and save a useful fact");
        let final_event = terminal(&rx).pop().unwrap();
        assert_eq!(
            final_event.kind,
            ChatEventKind::Completed,
            "{}",
            final_event.text
        );
        let requests = (0..4)
            .map(|_| {
                server
                    .requests
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap()
                    .1
            })
            .collect::<Vec<_>>();
        let field = if api == "responses" {
            "input"
        } else {
            "messages"
        };
        assert_eq!(requests[0]["tool_choice"], "auto");
        assert_eq!(requests[3]["tool_choice"], "none");
        let final_body = requests[3][field].to_string();
        for id in ["search", "read", "save"] {
            assert!(final_body.contains(id));
        }
        assert!(final_body.contains("历史 seed 中文"));
        assert!(!final_body.contains("hidden synthetic reasoning"));
        let saved =
            std::fs::read_to_string(std::path::Path::new(&p.directory).join("MEMORY.md")).unwrap();
        assert_eq!(saved, "remembered bounded 中文");
        let request = core.model_request(&task.id).unwrap();
        assert_eq!(core.database.history_calls(&request).unwrap(), 2);
        drop(task);
        let (next, rx) = start(&core, &id, "follow up");
        assert_eq!(terminal(&rx).pop().unwrap().kind, ChatEventKind::Completed);
        let replay = server
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .1;
        assert!(
            replay[field]
                .to_string()
                .contains("remembered bounded 中文")
        );
        // Prior lookups do not spend this turn's four-call allowance.
        let request = core.model_request(&next.id).unwrap();
        assert_eq!(core.database.history_calls(&request).unwrap(), 0);
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM memory_operations WHERE persona_id=?1",
                [&p.id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}
