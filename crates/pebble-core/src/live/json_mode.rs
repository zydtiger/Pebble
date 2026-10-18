//! The official DeepSeek Chat Completions API exposes JSON mode, not JSON Schema.
use super::*;

pub(super) fn prepare(chat: &ChatConnection, request: &ModelRequest, body: &mut Value) {
    if !chat.uses_json_object() {
        return;
    }
    if let Some(schema) = &request.response_schema {
        // A private call's contract differs from the ordinary frozen chat prefix.
        // Include the actual current-purpose schema even when tools are offered.
        // This is derived transport context, not a user event or a history rewrite.
        let required = &schema["required"];
        let instruction = format!(
            "For the final assistant response, return one JSON object with these required TOP-LEVEL fields: {required}. Match this current-purpose JSON Schema: {schema}. Return an instance, not the schema itself or only a nested object. This contract controls the final response shape for this request. Native tool calls remain available only when offered. Do not add Markdown fences or fields outside this schema."
        );
        body["messages"]
            .as_array_mut()
            .expect("chat messages")
            .push(json!({"role":"system","content":instruction}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_endpoint_uses_json_mode_with_the_current_purpose_and_keeps_tools_separate() {
        for endpoint in [
            "https://api.deepseek.com",
            "https://api.deepseek.com/v1",
            "https://api.deepseek.com.example.org/v1",
            "http://localhost/v1",
        ] {
            let chat = ChatConnection {
                model: "any-model".into(),
                base_url: endpoint.into(),
                api: "chat_completions".into(),
                context_len: 100000,
                max_output_tokens: 512,
                api_key: None,
                api_key_env: None,
                responses_schema_format: None,
                request: Default::default(),
            };
            let mut request: ModelRequest = serde_json::from_value(json!({"call_id":"call","session_id":"session","conversation_id":"room","trigger_message_id":"","event_cutoff":0,"protocol":"chat_completions","model":"any-model","items":[],"memory_tools":false,"memory_capable":true})).unwrap();
            let schema = crate::structured::private_schema(
                &serde_json::from_str::<Value>(crate::structured::DEFAULT_SCHEMA).unwrap(),
            )
            .unwrap();
            request.response_schema = Some(schema.clone());
            let original = request.items.clone();
            let official =
                endpoint == "https://api.deepseek.com" || endpoint == "https://api.deepseek.com/v1";
            let (_, body) = request_body(&chat, &request, 512).unwrap();
            assert_eq!(
                body["response_format"]["type"],
                if official {
                    "json_object"
                } else {
                    "json_schema"
                }
            );
            assert_eq!(
                body["messages"].as_array().unwrap().len(),
                usize::from(official)
            );
            if official {
                assert!(
                    body["messages"][0]["content"]
                        .as_str()
                        .unwrap()
                        .contains(&schema.to_string())
                );
                assert!(body["response_format"].get("json_schema").is_none());
            }
            assert_eq!(request.items.len(), original.len());
            request.delegation_tools = true;
            let (_, tools) = request_body(&chat, &request, 512).unwrap();
            assert!(tools.get("response_format").is_none());
            assert_eq!(tools["tool_choice"], "auto");
            request.response_schema = None;
            request.delegation_tools = false;
            request.memory_tools = true;
            let (_, maintenance) = request_body(&chat, &request, 512).unwrap();
            assert!(maintenance.get("response_format").is_none());
            assert_eq!(maintenance["messages"], json!([]));
            assert_eq!(maintenance["tool_choice"], "required");
        }
    }
}
