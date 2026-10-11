use super::*;
#[test]
fn complete_body_budget_includes_utf8_schema_native_state_and_both_dialects() {
    for (api, dialect) in [
        ("chat_completions", None),
        ("responses", None),
        ("responses", Some("response_format")),
    ] {
        let chat = ChatConnection {
            model: "test".into(),
            base_url: "http://localhost/v1".into(),
            api: api.into(),
            api_key: None,
            api_key_env: None,
            responses_schema_format: dialect.map(str::to_owned),
            context_len: 100_000,
            max_output_tokens: 77,
        };
        let request = ModelRequest {
            call_id: String::new(),
            session_id: String::new(),
            conversation_id: String::new(),
            trigger_message_id: String::new(),
            event_cutoff: 0,
            protocol: api.into(),
            model: "test".into(),
            items: vec![crate::ModelItem {
                id: String::new(),
                sequence: 0,
                payload: serde_json::to_vec(
                    &json!({"role":"system","content":"SOUL 你好 MEMORY","opaque_state":"秘密"}),
                )
                .unwrap(),
            }],
            response_schema: Some(
                json!({"type":"object","description":"模式".repeat(200),"properties":{"reply":{"type":"string"}}}),
            ),
            state_before: None,
            memory_tools: false,
            memory_capable: false,
        };
        let (_, body) = request_body(&chat, &request, 77).unwrap();
        assert_eq!(
            body[if api == "responses" {
                "max_output_tokens"
            } else {
                "max_tokens"
            }],
            77
        );
        assert!(body.get("context_len").is_none());
        let estimate = serde_json::to_vec(&body).unwrap().len() as u64 + 128;
        let mut budget = crate::ContextBudget {
            context_len: estimate + 77,
            max_output_tokens: 77,
        };
        budget.check_body(&body).unwrap();
        budget.context_len -= 1;
        assert!(matches!(
            budget.check_body(&body),
            Err(Error::ContextLimit(_))
        ));
        let mut plain = request.clone();
        plain.response_schema = None;
        budget
            .check_body(&request_body(&chat, &plain, 77).unwrap().1)
            .unwrap();
        assert!(estimate > body.to_string().chars().count() as u64 + 128);
    }
}
