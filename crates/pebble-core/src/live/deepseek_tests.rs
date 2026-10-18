use super::*;

#[test]
fn deepseek_json_mode_is_host_and_protocol_specific_and_keeps_the_local_contract() {
    let schema: Value =
        serde_json::from_str(include_str!("../../assets/response_schema.json")).unwrap();
    for (url, api, object) in [
        ("https://api.deepseek.com", "chat_completions", true),
        ("https://api.deepseek.com/v1/", "chat_completions", true),
        ("https://api.openai.com/v1", "chat_completions", false),
        ("http://localhost:30000/v1", "chat_completions", false),
        (
            "https://api.deepseek.com.example.org",
            "chat_completions",
            false,
        ),
        ("https://api.deepseek.com", "responses", false),
    ] {
        let chat: ChatConnection = serde_json::from_value(json!({
            "model":"deepseek-flash", "base_url":url, "api":api,
            "context_len":65536, "max_output_tokens":4096
        }))
        .unwrap();
        let mut request: ModelRequest = serde_json::from_value(json!({
            "call_id":"call", "session_id":"session", "conversation_id":"room",
            "trigger_message_id":"trigger", "event_cutoff":0, "protocol":api,
            "model":"deepseek-flash", "items":[], "response_schema":schema,
            "memory_tools":false, "memory_capable":false
        }))
        .unwrap();
        let (_, body) = request_body(&chat, &request, 4096).unwrap();
        let format = if api == "responses" {
            &body["text"]["format"]
        } else {
            &body["response_format"]
        };
        assert_eq!(
            format["type"],
            if object { "json_object" } else { "json_schema" }
        );
        if object {
            assert_eq!(format, &json!({"type":"json_object"}));
        }
        assert_eq!(request.response_schema.as_ref(), Some(&schema));
        assert!(
            crate::structured::candidate(
                &schema,
                r#"{"reply":"ok","state":{"affinity_user":20,"mood":"calm"}}"#
            )
            .is_ok()
        );
        assert!(
            crate::structured::candidate(
                &schema,
                r#"{"reply":"ok","state":{"affinity_user":101,"mood":"calm"}}"#
            )
            .is_err()
        );
        request.memory_tools = true;
        let (_, tools) = request_body(&chat, &request, 4096).unwrap();
        assert!(tools.get("response_format").is_none());
        request.memory_tools = false;
        request.response_schema = None;
        let (_, plain) = request_body(&chat, &request, 4096).unwrap();
        assert!(plain.get("response_format").is_none());
        assert!(plain.get("text").is_none());
    }
}
