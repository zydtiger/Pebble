use super::*;
#[test]
fn complete_body_budget_includes_utf8_schema_native_state_and_both_dialects() {
    for (api, dialect) in [
        ("chat_completions", None),
        ("responses", None),
        ("responses", Some("response_format")),
    ] {
        let chat = ChatConnection {
            request: json!({"temperature":0,"chat_template_kwargs":{"enable_thinking":false},"vendor":{"nested":[0,false,"你好"]}}).as_object().unwrap().clone(),
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
            image_root: Default::default(),
            recent_history: None,
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
            history_tools: false,
            history_cutoff: 0,
            memory_tools: false,
            memory_capable: false,
            delegation_tools: false,
            image_generation_tools: false,
            image_generation_backends: Vec::new(),
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
        assert_eq!(body["temperature"], 0);
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["vendor"]["nested"][0], 0);
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

#[test]
fn image_generation_backend_enum_contains_only_available_choices() {
    for api in ["chat_completions", "responses"] {
        let chat = ChatConnection {
            request: Default::default(),
            model: "test".into(),
            base_url: "https://api.example.com".into(),
            api: api.into(),
            api_key: None,
            api_key_env: None,
            responses_schema_format: None,
            context_len: 100_000,
            max_output_tokens: 77,
        };
        let mut request: ModelRequest = serde_json::from_value(json!({"call_id":"call","session_id":"session","conversation_id":"room","trigger_message_id":"trigger","event_cutoff":0,"protocol":api,"model":"test","items":[],"memory_tools":false,"memory_capable":false,"image_generation_tools":true})).unwrap();
        for backend in ["studio", "chatgpt"] {
            request.image_generation_backends = vec![backend.into()];
            let (_, body) = request_body(&chat, &request, 77).unwrap();
            let tool = body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| {
                    v["name"] == "generate_image" || v["function"]["name"] == "generate_image"
                })
                .unwrap();
            let parameters = if api == "responses" {
                &tool["parameters"]
            } else {
                &tool["function"]["parameters"]
            };
            assert_eq!(
                parameters["properties"]["backend"]["enum"],
                json!([backend])
            );
        }
    }
}

#[test]
fn maintenance_exposes_only_memory_while_replaying_all_native_tool_evidence() {
    for api in ["chat_completions", "responses"] {
        let chat = ChatConnection {
            request: Default::default(),
            model: "test".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            api: api.into(),
            api_key: None,
            api_key_env: None,
            responses_schema_format: None,
            context_len: 100000,
            max_output_tokens: 512,
        };
        let mut request:ModelRequest=serde_json::from_value(json!({"call_id":"maintenance","session_id":"session","conversation_id":"room","trigger_message_id":"","event_cutoff":0,"protocol":api,"model":"test","items":[],"memory_tools":true,"memory_capable":true})).unwrap();
        for name in [
            "generate_image",
            "delegate_task",
            "search_history",
            "read_history",
        ] {
            let call = if api == "responses" {
                json!({"type":"function_call","call_id":name,"name":name,"arguments":"{}"})
            } else {
                json!({"role":"assistant","content":null,"tool_calls":[{"id":name,"type":"function","function":{"name":name,"arguments":"{}"}}]})
            };
            let result = crate::memory::result(api, name, "{\"ok\":true}");
            crate::memory::append(&mut request, &[call, result]).unwrap();
        }
        let body = request_body(&chat, &request, 512).unwrap().1;
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        assert_eq!(body["tool_choice"], "required");
    }
}
