mod live_support;
use live_support::*;
use pebble_core::ChatEventKind;

#[test]
#[ignore = "requires explicit DeepSeek credentials; uses isolated synthetic data"]
fn deepseek_chat_accepts_state_and_replays_after_restart() {
    assert!(std::env::var("PEBBLE_DEEPSEEK_CHAT_KEY").is_ok());
    let (root, core, id) = isolated();
    std::fs::write(root.path().join("config.toml"),
        "[defaults.chat]\nmodel='deepseek-flash'\nbase_url='https://api.deepseek.com'\napi='chat_completions'\napi_key_env='PEBBLE_DEEPSEEK_CHAT_KEY'\ncontext_len=262144\nmax_output_tokens=4096\n"
    ).unwrap();
    let (task, rx) = start(
        &core,
        &id,
        "Reply with exactly PEBBLE_OK. Keep your state unchanged.",
    );
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    let first = core.model_request(&task.id).unwrap();
    assert!(first.response_schema.is_some());
    assert!(
        core.messages(&id)
            .unwrap()
            .last()
            .unwrap()
            .content
            .contains("PEBBLE_OK")
    );
    drop((task, core));
    let core = reopen(root.path());
    let (task, rx) = start(
        &core,
        &id,
        "Reply with exactly PEBBLE_REPLAY_OK. Keep your state unchanged.",
    );
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    let second = core.model_request(&task.id).unwrap();
    assert_eq!(first.session_id, second.session_id);
    assert_eq!(
        second.state_before.unwrap(),
        serde_json::json!({"affinity_user":20,"mood":"calm"})
    );
    assert!(
        core.messages(&id)
            .unwrap()
            .last()
            .unwrap()
            .content
            .contains("PEBBLE_REPLAY_OK")
    );
}
