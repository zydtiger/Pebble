//! Opt-in real local-child checks. Synthetic data only; no personal event contents.
use super::{adapter::Adapter, *};
use serde_json::json;
use std::time::{Duration, Instant};

fn exercise(writable: bool, prompt: &str) -> (String, Vec<String>) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("fixture.txt"), "PEBBLE_SYNTHETIC_42\n").unwrap();
    let settings = CodexSettings {
        enabled: true,
        writable,
        ..Default::default()
    };
    let settings = settings::TaskSettings::new(&settings, root.path());
    let mut adapter = Adapter::start(&settings).unwrap();
    let start = Instant::now();
    let thread=adapter.rpc("thread/start",json!({"cwd":settings.cwd().unwrap(),"sandbox":if writable{"workspace-write"}else{"read-only"},"approvalPolicy":"on-request","approvalsReviewer":"auto_review","developerInstructions":"This is a bounded synthetic integration probe. Do not access personal calendar events, personal notes, unrelated files or send external messages. Do not delegate to subagents. Report actual sources, artifacts and limitations."})).unwrap();
    println!("effective_model={}", thread["model"]);
    let id = thread["thread"]["id"].as_str().unwrap();
    let turn = adapter
        .rpc(
            "turn/start",
            json!({"threadId":id,"input":[{"type":"text","text":prompt,"text_elements":[]}]}),
        )
        .unwrap();
    let turn_id = turn["turn"]["id"].as_str().unwrap();
    let mut answer = String::new();
    let mut actions = Vec::new();
    loop {
        if start.elapsed() > Duration::from_secs(180) {
            let _ = adapter.rpc("turn/interrupt", json!({"threadId":id,"turnId":turn_id}));
            panic!("bounded real Codex probe timed out");
        }
        let Some(event) = adapter.event().unwrap() else {
            continue;
        };
        if let Some(request) = event.get("id") {
            let method = event["method"].as_str().unwrap_or_default();
            if method.ends_with("requestApproval") {
                adapter
                    .send(json!({"id":request,"result":{"decision":"decline"}}))
                    .unwrap();
            } else {
                adapter.send(json!({"id":request,"error":{"code":-32601,"message":"No interactive permission is granted by this synthetic probe."}})).unwrap();
            }
            actions.push(format!("declined interaction {method}"));
            continue;
        }
        if event["method"] == "item/completed" {
            let item = &event["params"]["item"];
            if item["type"] == "agentMessage" && item["phase"] != "commentary" {
                answer = item["text"].as_str().unwrap_or_default().into();
            }
            if ["webSearch", "commandExecution", "fileChange", "mcpToolCall"]
                .contains(&item["type"].as_str().unwrap_or_default())
            {
                actions.push(item["type"].as_str().unwrap().into());
            }
        }
        if event["method"] == "turn/completed" {
            println!(
                "status={} elapsed_seconds={} actions={actions:?}\n{answer}",
                event["params"]["turn"]["status"],
                start.elapsed().as_secs_f32()
            );
            assert_eq!(event["params"]["turn"]["status"], "completed");
            break;
        }
    }
    if writable {
        assert_eq!(
            std::fs::read_to_string(root.path().join("answer.txt"))
                .unwrap()
                .trim(),
            "PEBBLE_SYNTHETIC_42"
        );
    }
    (answer, actions)
}
#[test]
#[ignore = "requires installed authenticated local Codex; read-only metadata"]
fn real_local_capabilities() {
    let root = tempfile::tempdir().unwrap();
    let settings = settings::TaskSettings::new(&CodexSettings::default(), root.path());
    let capabilities = super::adapter::inspect(&settings).unwrap();
    println!("{capabilities:?}");
    assert!(capabilities.authenticated);
    assert!(!capabilities.models.is_empty());
}
#[test]
#[ignore = "requires installed authenticated local Codex; bounded official web lookup"]
fn real_read_only_web_task() {
    let (answer, actions) = exercise(
        false,
        "Use web search to verify the current stable Rust version from the official rust-lang.org website. Return the version and a precise source URL; keep it to two sentences. Do not use local files or perform actions.",
    );
    assert!(actions.iter().any(|a| a == "webSearch"));
    assert!(answer.contains("https://"));
}
#[test]
#[ignore = "requires installed authenticated local Codex; status only, no personal calendar contents"]
fn real_calendar_readiness_only() {
    let (answer, _) = exercise(
        false,
        "Find and read the existing mac-personal-planning skill instructions, if available, to discover the configured calendar CLI. Verify ONLY its authorization/readiness status with icalctl status --json. Do not list calendars, search/read events, read notes or make changes. Return the actual authorization result or exact capability/permission limitation in two sentences.",
    );
    assert!(!answer.is_empty());
}
#[test]
#[ignore = "requires installed authenticated local Codex; scoped synthetic workspace write"]
fn real_scoped_file_task() {
    let (answer, actions) = exercise(
        true,
        "Read fixture.txt in this synthetic workspace and write answer.txt with exactly the same line PEBBLE_SYNTHETIC_42. This file write is explicitly authorized. Do not touch other paths or use the network. Return the actual artifact path and one sentence describing verification.",
    );
    assert!(!answer.is_empty());
    assert!(
        actions
            .iter()
            .any(|a| a == "fileChange" || a == "commandExecution")
    );
}

#[test]
#[ignore = "status/doctor only; no authorization request or calendar contents"]
fn real_calendar_launch_diagnostics_only() {
    let (answer, _) = exercise(
        false,
        "Diagnose ONLY the launch/authorization context of the existing icalctl CLI. Run icalctl status --json and icalctl doctor --json, then report their exact authorization and launch/TCC diagnostics. Do not request authorization, list calendars, search/read events, read notes, alter settings or files, or retry outside this read-only sandbox. Explain whether diagnostics establish sandbox denial or responsible-process TCC; explicitly say if inconclusive.",
    );
    assert!(!answer.is_empty());
}
