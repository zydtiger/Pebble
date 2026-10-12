// Shared deterministic app-server fixtures for delegation integration tests.
#![allow(dead_code)]
use pebble_core::{Core, DelegatedTask};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};
pub fn configure(core: &Core, workspace: &Path, mode: &str) {
    std::fs::write(
        workspace.join("fake-options.json"),
        json!({"mode":mode,"artifact":true}).to_string(),
    )
    .unwrap();
    let script = workspace.join("fake-app-server");
    std::fs::write(&script, include_str!("app_server.py")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    core.save_codex_settings(&pebble_core::CodexSettings {
        enabled: true,
        executable: script.display().to_string(),
        writable: false,
        model: None,
        effort: None,
    })
    .unwrap();
}
pub fn assert_fake(core: &Core, workspace: &Path) {
    assert_eq!(
        core.codex_settings().unwrap().executable,
        workspace.join("fake-app-server").display().to_string(),
        "Isolated fixture must not dispatch a real Codex executable"
    );
}
pub fn wait(core: &Core, predicate: impl Fn(&[DelegatedTask]) -> bool) -> Vec<DelegatedTask> {
    let until = Instant::now() + Duration::from_secs(12);
    loop {
        let tasks = core.delegated_tasks().unwrap();
        if predicate(&tasks) {
            return tasks;
        }
        assert!(Instant::now() < until, "tasks timed out: {tasks:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub fn settled(core: &Core) -> DelegatedTask {
    wait(core, |t| {
        t.len() == 1
            && !t[0].state.active()
            && t[0].report_state != "pending"
            && t[0].report_state != "queued"
    })
    .remove(0)
}
pub fn method_count(workspace: &Path, method: &str) -> usize {
    std::fs::read_to_string(workspace.join("fake-methods.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|s| serde_json::from_str::<Value>(s).unwrap()["method"] == method)
        .count()
}
pub fn tool(api: &str, key: &str) -> String {
    tool_for(api, key, None)
}
pub fn tool_for(api: &str, key: &str, task: Option<&str>) -> String {
    let args=json!({"title":"Synthetic lookup","instructions":"Look up synthetic evidence and save the actual result; no personal data.","request_key":key,"task_id":task}).to_string();
    if api == "responses" {
        format!(
            "data: {}\n\n",
            json!({"type":"response.completed","response":{"status":"completed","output":[{"id":"native-delegation","type":"function_call","status":"completed","call_id":format!("delegate-{key}"),"name":"delegate_task","arguments":args}]}})
        )
    } else {
        format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":format!("delegate-{key}"),"type":"function","function":{"name":"delegate_task","arguments":args}}]},"finish_reason":null}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})
        )
    }
}
