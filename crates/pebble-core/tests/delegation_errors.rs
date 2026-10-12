mod delegation_support;
mod live_support;
use delegation_support::{
    assert_fake, configure as codex_configure, method_count, settled, tool, wait,
};
use live_support::*;
use pebble_core::DelegationState;

#[test]
fn definitive_turn_error_ends_progress_without_a_completion_notification() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "fatal-error")),
        stream(success("Started separately.")),
        stream(success("The task failed; no result was returned.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "error-fatal");
    assert_fake(&core, workspace.path());
    let (handle, events) = start(&core, &persona, "Run the synthetic fatal-error fixture.");
    terminal(&events);
    let failed = wait(&core, |tasks| {
        tasks.len() == 1 && tasks[0].state == DelegationState::Failed
    })
    .remove(0);
    assert!(failed.activity.is_empty() && failed.request.is_none());
    assert!(failed.detail.contains("Synthetic turn error"));
    assert!(!failed.detail.contains("PRIVATE_RAW_STACK"));
    assert!(failed.result.is_empty());
    let saved = settled(&core);
    assert_eq!(saved.state, DelegationState::Failed);
    assert_eq!(saved.report_state, "completed");
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    drop(handle);
    drop(core);
    let reopened = pebble_core::Core::open(root.path()).unwrap();
    let restored = settled(&reopened);
    assert_eq!(restored.state, DelegationState::Failed);
    assert_eq!(restored.detail, saved.detail);
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
}

#[test]
fn delayed_final_frames_preserve_evidence_and_the_original_error() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "delayed-error")),
        stream(success("Started separately.")),
        stream(success("The task failed with saved partial evidence.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "error-delayed");
    assert_fake(&core, workspace.path());
    let (handle, events) = start(
        &core,
        &persona,
        "Run the synthetic delayed-final-frame fixture.",
    );
    terminal(&events);
    let failed = wait(&core, |tasks| {
        tasks.len() == 1 && tasks[0].state == DelegationState::Failed
    })
    .remove(0);
    assert!(failed.activity.is_empty() && failed.result.is_empty());
    std::fs::write(workspace.path().join("release"), "synthetic").unwrap();
    let saved = settled(&core);
    assert_eq!(saved.state, DelegationState::Failed);
    assert!(saved.result.contains("Saved partial evidence"));
    assert!(saved.detail.contains("Synthetic turn error"));
    assert!(saved.detail.contains("citation markers"));
    assert_eq!(saved.detail.matches("citation markers").count(), 1);
    assert!(!saved.result.contains('\u{e200}'));
    assert!(!saved.detail.contains("PRIVATE_RAW_STACK"));
    assert_eq!(saved.links[0].target, "https://example.org/partial");
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    drop(handle);
}

#[test]
fn retryable_error_stays_active_and_stale_errors_or_failed_tools_do_not_end_execution() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "retry-error")),
        stream(success("Started separately.")),
        stream(success("The retry completed with evidence.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "error-retry");
    assert_fake(&core, workspace.path());
    let (handle, events) = start(&core, &persona, "Run the synthetic retry fixture.");
    terminal(&events);
    let retrying = wait(&core, |tasks| {
        tasks.len() == 1 && tasks[0].activity.contains("retrying")
    })
    .remove(0);
    assert_eq!(retrying.state, DelegationState::Running);
    assert!(
        retrying.detail.contains("will retry") && retrying.detail.contains("Synthetic turn error")
    );
    assert!(!retrying.detail.contains("PRIVATE_RAW_STACK"));
    std::fs::write(workspace.path().join("release"), "synthetic").unwrap();
    let saved = settled(&core);
    assert_eq!(saved.state, DelegationState::Completed);
    assert!(saved.activity.is_empty());
    assert!(!saved.detail.contains("Synthetic turn error"));
    assert_eq!(method_count(workspace.path(), "turn/start"), 1);
    drop(handle);
}
