use super::*;
use serde_json::json;
#[test]
fn protected_fields_protocol_types_and_known_conflicts() {
    let fields = [
        "model",
        "input",
        "messages",
        "stream",
        "stream_options",
        "n_predict",
        "max_new_tokens",
        "max_tokens",
        "max_completion_tokens",
        "max_output_tokens",
        "response_format",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "functions",
        "function_call",
        "store",
        "include",
        "previous_response_id",
        "conversation",
        "instructions",
        "truncation",
        "background",
        "n",
        "grammar",
        "json_schema",
        "parse_tool_calls",
        "reasoning_format",
        "response_contract",
        "memory_tools",
    ];
    for api in ["chat_completions", "responses"] {
        for key in fields {
            assert!(
                validate(&Map::from_iter([(key.into(), json!(false))]), api).is_err(),
                "{api}:{key}"
            );
        }
    }
    for (api, valid) in [
        (
            "chat_completions",
            json!({"reasoning_effort":"low","stop":["END"],"logprobs":true,"top_logprobs":0}),
        ),
        (
            "responses",
            json!({"reasoning":{"effort":"none"},"text":{"verbosity":"low"}}),
        ),
    ] {
        assert!(validate(valid.as_object().unwrap(), api).is_ok());
    }
    for (api, invalid) in [
        ("chat_completions", json!({"reasoning":{"effort":"low"}})),
        ("responses", json!({"reasoning_effort":"low"})),
        ("responses", json!({"text":{"format":{}}})),
        ("responses", json!({"stop":"END"})),
        ("chat_completions", json!({"top_logprobs":2})),
        (
            "chat_completions",
            json!({"thinking":{"type":"disabled","budget_tokens":10}}),
        ),
    ] {
        assert!(
            validate(invalid.as_object().unwrap(), api).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn thinking_controls_combine_dimensions_but_reject_contradictions() {
    for valid in [
        json!({"reasoning_effort":"backend-specific","chat_template_kwargs":{"enable_thinking":true,"thinking_budget":100}}),
        json!({"reasoning_effort":"none","chat_template_kwargs":{"enable_thinking":false}}),
    ] {
        assert!(
            validate(valid.as_object().unwrap(), "chat_completions").is_ok(),
            "{valid}"
        );
    }
    for invalid in [
        json!({"reasoning_effort":"none","chat_template_kwargs":{"enable_thinking":true}}),
        json!({"chat_template_kwargs":{"enable_thinking":false,"thinking_budget":100}}),
        json!({"chat_template_kwargs":{"enable_thinking":false,"thinking_budget":"100"}}),
        json!({"reasoning_effort":"low","chat_template_kwargs":{"reasoning_effort":"high"}}),
        json!({"chat_template_kwargs":{"reasoning":{"effort":false}}}),
        json!({"chat_template_kwargs":{"thinking":{"type":"disabled","budget_tokens":5}}}),
    ] {
        assert!(
            validate(invalid.as_object().unwrap(), "chat_completions").is_err(),
            "{invalid}"
        );
    }
}
