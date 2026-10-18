use super::*;

#[test]
#[ignore = "requires explicit DeepSeek credentials; sends synthetic classifier input only"]
fn deepseek_participation_accepts_true_and_false() {
    assert!(std::env::var("PEBBLE_DEEPSEEK_CLASSIFIER_KEY").is_ok());
    let config: ClassifierConnection = serde_json::from_value(json!({
        "model":"deepseek-flash", "base_url":"https://api.deepseek.com",
        "api":"chat_completions", "api_key_env":"PEBBLE_DEEPSEEK_CLASSIFIER_KEY",
        "context_len":16384, "max_output_tokens":64,
        "request":{"temperature":0,"thinking":{"type":"disabled"}}
    }))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for (message, expected) in [
        ("Test Persona, please say hello to me.", true),
        (
            "Test Persona, please stay silent and give me space. Do not reply.",
            false,
        ),
    ] {
        let input = json!({
            "persona":{"id":"synthetic","name":"Test Persona","soul":"You are a friendly companion.","memory":""},
            "latest_trigger":{"message_id":"synthetic-message","text":message},
            "recent_group_transcript":[{"message_id":"synthetic-message","speaker":{"kind":"user","name":"User"},"text":message}]
        });
        assert_eq!(
            runtime
                .block_on(classify(&config, &input.to_string(), "initial"))
                .unwrap(),
            expected
        );
    }
}
