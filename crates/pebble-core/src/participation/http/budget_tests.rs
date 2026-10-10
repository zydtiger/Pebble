use super::*;
fn config(api: &str, dialect: Option<&str>) -> ClassifierConnection {
    ClassifierConnection {
        connection: crate::ChatConnection {
            model: "classifier".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            api: api.into(),
            api_key: None,
            api_key_env: None,
            responses_schema_format: dialect.map(str::to_owned),
            context_len: 100_000,
            max_output_tokens: 91,
        },
        request_profile: (api == "chat_completions").then(|| "qwen_non_thinking".into()),
    }
}
#[test]
fn every_phase_has_complete_budget_and_configured_output_before_http() {
    for (api, dialect) in [
        ("chat_completions", None),
        ("responses", None),
        ("responses", Some("response_format")),
    ] {
        for phase in ["initial", "reassess", "intent", "permission"] {
            let mut config = config(api, dialect);
            let input = json!({"persona":{"id":"target","name":"名字"},"latest_trigger":{"message_id":"trigger","text":"问题"},"recent_real_user_transcript":[{"text":"remain silent"}],"silence_constraints":["no reply"],"latest_user_message":{"content":"大家？"}});
            let body = budgeted_body(&config, &input.to_string(), phase).unwrap();
            let estimated = serde_json::to_vec(&body).unwrap().len() as u64 + 128;
            assert_eq!(
                body[if api == "responses" {
                    "max_output_tokens"
                } else {
                    "max_tokens"
                }],
                91
            );
            assert!(body.get("context_len").is_none());
            config.connection.context_len = estimated + 91;
            budgeted_body(&config, &input.to_string(), phase).unwrap();
            config.connection.context_len -= 1;
            // classify must stop here despite the unreachable endpoint.
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let error = rt
                .block_on(classify(&config, &input.to_string(), phase))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("mandatory input") && error.contains("before HTTP"),
                "{phase}: {error}"
            );
            let mut oversized = input.clone();
            oversized["latest_trigger"]["text"] = json!("大".repeat(10000));
            assert!(budgeted_body(&config, &oversized.to_string(), phase).is_err());
        }
    }
}
#[test]
fn pruning_retains_trigger_and_same_round_predecessor_by_identity_and_permission_silence() {
    for api in ["chat_completions", "responses"] {
        for phase in ["initial", "reassess"] {
            let mut config = config(api, None);
            let mut input = json!({"persona":{"id":"target"},"latest_trigger":{"message_id":"trigger","text":"latest 你好"},"_required_message_ids":["trigger","accepted"],"recent_persona_events":[{"message_id":"old-private","text":"旧".repeat(3000)}],"recent_group_transcript":[{"message_id":"old","text":"旧".repeat(3000)},{"message_id":"trigger","text":"latest 你好"},{"message_id":"accepted","text":"newer accepted predecessor"}]});
            let mut mandatory = input.clone();
            mandatory
                .as_object_mut()
                .unwrap()
                .remove("_required_message_ids");
            mandatory["recent_persona_events"] = json!([]);
            mandatory["recent_group_transcript"]
                .as_array_mut()
                .unwrap()
                .remove(0);
            let body = request_body(&config, &mandatory, phase).unwrap();
            config.connection.context_len =
                serde_json::to_vec(&body).unwrap().len() as u64 + 128 + 91;
            let body = budgeted_body(&config, &input.to_string(), phase).unwrap();
            let text = if api == "responses" {
                &body["input"][1]["content"][0]["text"]
            } else {
                &body["messages"][1]["content"]
            };
            let final_input: Value = serde_json::from_str(text.as_str().unwrap()).unwrap();
            assert_eq!(final_input, mandatory);
            input["recent_real_user_transcript"] =
                json!([{"text":"old explicit silence".repeat(1000)}]);
            assert!(budgeted_body(&config, &input.to_string(), "permission").is_err());
        }
    }
}
