mod delegation_support;
mod live_support;
mod responses_support;
use delegation_support::{
    assert_fake, configure as codex_configure, method_count, settled, tool, tool_for, wait,
};
use live_support::*;
use pebble_core::Core;
use serde_json::{Value, json};
use std::path::Path;

fn reply(api: &str, text: &str) -> String {
    if api == "responses" {
        responses_support::success(text)
    } else {
        success(text)
    }
}
fn record(root: &Path) -> Value {
    let db = rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap();
    let record: String = db
        .query_row("SELECT record FROM delegated_tasks", [], |r| r.get(0))
        .unwrap();
    serde_json::from_str(&record).unwrap()
}
#[test]
fn native_and_card_followups_preserve_original_actual_continuation_and_objective_across_reload() {
    for api in ["chat_completions", "responses"] {
        let initial_user =
            "Inspect only the synthetic project documentation; no external messages.";
        let native_user =
            "Continue with its synthetic usage examples, preserving the original limits.";
        let card_user = "  Now summarize the same synthetic documentation evidence.  ";
        let objective = "Look up synthetic evidence and save the actual result; no personal data.";
        let initial = Server::memory(vec![
            stream(tool(api, "initial")),
            stream(reply(api, "Task started.")),
            stream(reply(api, "Initial evidence explained.")),
        ]);
        let (root, core, persona) = isolated();
        configure(root.path(), &initial.url, "mock", api, "");
        let workspace = tempfile::tempdir().unwrap();
        codex_configure(&core, workspace.path(), "complete");
        let initial_sources = (0..32)
            .map(|i| format!("https://example.org/initial/{i}"))
            .collect::<Vec<_>>();
        std::fs::write(
            workspace.path().join("fake-options.json"),
            json!({"mode":"complete","artifact":false,"text":initial_sources.join(" ")})
                .to_string(),
        )
        .unwrap();
        assert_fake(&core, workspace.path());
        let (initial_handle, events) = start(&core, &persona, initial_user);
        terminal(&events);
        let task = settled(&core);
        assert_eq!(task.links.len(), 32);
        let thread = task.thread_id.clone().unwrap();
        let saved = record(root.path());
        assert_eq!(saved["original_request"], initial_user);
        assert_eq!(saved["followup_request"], "");
        let later = Server::memory(vec![
            stream(tool_for(api, "native-followup", Some(&task.id))),
            stream(reply(api, "Continuing the saved task.")),
            stream(reply(api, "Continuation evidence explained.")),
            stream(reply(api, "Card continuation evidence explained.")),
        ]);
        let mut changed_settings = core.codex_settings().unwrap();
        configure(root.path(), &later.url, "mock", api, "");
        // A later app selection must not change this task's frozen execution scope.
        let other_workspace = tempfile::tempdir().unwrap();
        changed_settings.workspace = other_workspace.path().display().to_string();
        changed_settings.writable = true;
        changed_settings.model = Some("later-app-selection".into());
        changed_settings.effort = Some("low".into());
        core.save_codex_settings(&changed_settings).unwrap();
        assert_fake(&core, workspace.path());
        std::fs::write(workspace.path().join("fake-options.json"), json!({"mode":"followup-gated","text":"Native continuation evidence https://example.org/native-current"}).to_string()).unwrap();
        let (native_handle, events) = start(&core, &persona, native_user);
        terminal(&events);
        let empty = wait(&core, |t| t[0].turn_id.as_deref() == Some("turn-2")).remove(0);
        assert!(empty.links.is_empty() && empty.result.is_empty());
        std::fs::write(workspace.path().join("release"), "synthetic").unwrap();
        let followed = wait(&core, |t| {
            t[0].turn_id.as_deref() == Some("turn-2") && t[0].report_state == "completed"
        })
        .remove(0);
        assert_eq!(followed.thread_id.as_deref(), Some(thread.as_str()));
        assert_eq!(followed.links.len(), 2);
        assert!(
            followed
                .links
                .iter()
                .any(|l| l.target == "https://example.org/native-current")
        );
        assert!(
            followed
                .links
                .iter()
                .any(|l| l.target.ends_with("followup-turn-2.txt"))
        );
        assert!(
            !followed
                .links
                .iter()
                .any(|l| l.target.contains("/initial/"))
        );
        let saved = record(root.path());
        assert_eq!(saved["original_request"], initial_user);
        assert_eq!(saved["followup_request"], native_user);
        assert_eq!(saved["instructions"], objective);
        assert_eq!(
            saved["reports"].as_array().unwrap().last().unwrap()["input"]["sources_and_artifacts"],
            json!(followed.links)
        );
        drop(native_handle);
        drop(initial_handle);
        drop(core);
        let reopened = Core::open(root.path()).unwrap();
        assert_fake(&reopened, workspace.path());
        let saved = record(root.path());
        assert_eq!(saved["original_request"], initial_user);
        assert_eq!(saved["followup_request"], native_user);
        std::fs::remove_file(workspace.path().join("release")).unwrap();
        std::fs::write(workspace.path().join("fake-options.json"), json!({"mode":"followup-gated","text":"Card continuation evidence https://example.org/card-current"}).to_string()).unwrap();
        reopened.followup_delegation(&task.id, card_user).unwrap();
        let empty = wait(&reopened, |t| t[0].turn_id.as_deref() == Some("turn-3")).remove(0);
        assert!(empty.links.is_empty() && empty.result.is_empty());
        std::fs::write(workspace.path().join("release"), "synthetic").unwrap();
        let followed = wait(&reopened, |t| {
            t[0].turn_id.as_deref() == Some("turn-3") && t[0].report_state == "completed"
        })
        .remove(0);
        assert_eq!(followed.thread_id.as_deref(), Some(thread.as_str()));
        let saved = record(root.path());
        assert_eq!(saved["original_request"], initial_user);
        assert_eq!(saved["followup_request"], card_user);
        assert_eq!(saved["instructions"], card_user.trim());
        assert_eq!(followed.links.len(), 2);
        assert!(
            followed
                .links
                .iter()
                .any(|l| l.target == "https://example.org/card-current")
        );
        assert!(
            followed
                .links
                .iter()
                .any(|l| l.target.ends_with("followup-turn-3.txt"))
        );
        let reports = saved["reports"].as_array().unwrap();
        let evidence = &reports.last().unwrap()["input"]["sources_and_artifacts"];
        assert_eq!(evidence, &json!(followed.links));
        assert!(
            !evidence.to_string().contains("native-current")
                && !evidence.to_string().contains("/initial/")
        );
        let calls = std::fs::read_to_string(workspace.path().join("fake-methods.jsonl"))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str::<Value>(s).unwrap())
            .collect::<Vec<_>>();
        let starts = calls
            .iter()
            .filter(|v| v["method"] == "turn/start")
            .collect::<Vec<_>>();
        assert_eq!(starts.len(), 3);
        let canonical_workspace = workspace
            .path()
            .canonicalize()
            .unwrap()
            .display()
            .to_string();
        for (index, call) in starts.iter().enumerate() {
            assert_eq!(call["params"]["threadId"], thread);
            assert!(call["params"].get("effort").is_none());
            let input: Value =
                serde_json::from_str(call["params"]["input"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(input["original_user_request"], initial_user);
            assert_eq!(
                input["current_user_followup"],
                ["", native_user, card_user][index]
            );
            assert_eq!(
                input["delegated_objective"],
                [objective, objective, card_user.trim()][index]
            );
            assert_eq!(input["workspace"], canonical_workspace);
            assert_eq!(input["workspace_writable"], false);
            assert!(
                input["runtime_time"]["local_date_time_and_offset"]
                    .as_str()
                    .is_some_and(|t| !t.is_empty())
            );
        }
        for resume in calls.iter().filter(|v| v["method"] == "thread/resume") {
            assert_eq!(resume["params"]["threadId"], thread);
            assert_eq!(resume["params"]["sandbox"], "read-only");
            assert!(resume["params"].get("model").is_none());
        }
        assert_eq!(method_count(workspace.path(), "thread/start"), 1);
        assert_eq!(method_count(workspace.path(), "thread/resume"), 2);
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        let history: Vec<String> = db
            .prepare("SELECT record FROM delegation_runs ORDER BY generation")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(history.len(), 2);
        for (index, snapshot) in history.iter().enumerate() {
            let prior: Value = serde_json::from_str(snapshot).unwrap();
            assert_eq!(prior["original_request"], initial_user);
            assert_eq!(prior["followup_request"], ["", native_user][index]);
            assert_eq!(prior["links"].as_array().unwrap().len(), [32, 2][index]);
            if index == 0 {
                assert_eq!(prior["links"], json!(task.links));
            } else {
                assert!(prior["links"].to_string().contains("native-current"));
            }
        }
        drop(db);
        drop(reopened);
        let final_reload = Core::open(root.path()).unwrap();
        let saved = record(root.path());
        assert_eq!(saved["original_request"], initial_user);
        assert_eq!(saved["followup_request"], card_user);
        assert_eq!(
            final_reload.delegated_tasks().unwrap()[0]
                .thread_id
                .as_deref(),
            Some(thread.as_str())
        );
        assert_eq!(method_count(workspace.path(), "turn/start"), 3);
    }
}
