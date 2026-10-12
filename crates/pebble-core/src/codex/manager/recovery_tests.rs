use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

fn admitted_core(
    state: DelegationState,
    thread: Option<&str>,
) -> (tempfile::TempDir, crate::Core, DelegatedTask, Value) {
    let root = tempfile::tempdir().unwrap();
    let core = crate::Core::open(root.path()).unwrap();
    let workspace = root.path().join("synthetic-workspace");
    std::fs::create_dir(&workspace).unwrap();
    let executable = workspace.join("fake-app-server");
    std::fs::write(
        &executable,
        include_str!("../../../tests/delegation_support/app_server.py"),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let settings = CodexSettings {
        executable: executable.display().to_string(),
        workspace: workspace.display().to_string(),
        model: Some("frozen-fixture-model".into()),
        effort: Some("high".into()),
        ..Default::default()
    };
    let persona = core.workspace().unwrap().personas.remove(0);
    let queued = core
        .database
        .enqueue_turn(&persona, "Inspect only this synthetic workspace.")
        .unwrap();
    let origin = core.database.activate_turn(&persona, &queued).unwrap();
    let request = core.database.request(&origin.call_id).unwrap();
    let (ack, task) = core
        .database
        .prepare_delegation(
            &request,
            &super::super::tool::Call {
                id: "accepted-boundary".into(),
                arguments: super::super::tool::Arguments {
                    title: "Accepted synthetic work".into(),
                    instructions: "Inspect the synthetic workspace".into(),
                    request_key: "shared".into(),
                    task_id: None,
                },
            },
            &[],
            &settings,
        )
        .unwrap();
    let task = task.unwrap();
    core.database
        .update_task(&task, |t| {
            t.state = state;
            t.thread_id = thread.map(str::to_owned);
            Ok(())
        })
        .unwrap();
    let saved = core.database.task(&task).unwrap();
    assert!(!saved.dispatch_started && saved.turn_id.is_none());
    assert!(Path::new(&saved.settings.executable).starts_with(root.path()));
    // Current app selections must not replace the accepted task's frozen configuration.
    std::fs::write(root.path().join("config.toml"), "[codex]\nenabled = false\nexecutable = 'must-not-be-selected'\nmodel = 'later-app-model'\neffort = 'low'\n").unwrap();
    (root, core, saved, ack)
}
fn accepted_before_dispatch(
    state: DelegationState,
    thread: Option<&str>,
) -> (tempfile::TempDir, DelegatedTask, Value) {
    let (root, core, task, ack) = admitted_core(state, thread);
    // No manager dispatch, no app-server process and no completed persona ack is fabricated.
    drop(core);
    (root, task, ack)
}
fn settled(core: &crate::Core, id: &str) -> DelegatedTask {
    let until = Instant::now() + Duration::from_secs(12);
    loop {
        let task = core.database.task(id).unwrap();
        if !task.state.active() && task.report_state == "completed" {
            return task;
        }
        assert!(Instant::now() < until, "recovery timed out: {task:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn reopen(root: &Path) -> crate::Core {
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        match crate::Core::open(root) {
            Ok(core) => return core,
            Err(Error::AlreadyOpen) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("isolated root did not reopen: {error}"),
        }
    }
}
fn methods(workspace: &Path) -> Vec<Value> {
    std::fs::read_to_string(workspace.join("fake-methods.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn reopen_dispatches_accepted_never_submitted_work_once_with_frozen_identity_and_scope() {
    for (state, thread) in [
        (DelegationState::Queued, None),
        (DelegationState::Running, None),
        (DelegationState::Running, Some("existing-pre-turn-thread")),
    ] {
        let (root, prior, ack) = accepted_before_dispatch(state, thread);
        let core = reopen(root.path());
        let task = settled(&core, &prior.id);
        assert_eq!(task.state, DelegationState::Completed);
        assert_eq!(task.submission, prior.submission);
        assert_eq!(task.settings, prior.settings);
        assert_eq!(task.reports.len(), 1);
        assert_eq!(task.reports[0].input["result"], task.result);
        assert!(task.result.contains("Verified synthetic"));
        let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
        let stored_ack: String = db
            .query_row(
                "SELECT result FROM delegation_submissions WHERE task_id=?1",
                [&prior.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(serde_json::from_str::<Value>(&stored_ack).unwrap(), ack);
        let calls = methods(Path::new(&prior.settings.workspace));
        let starts: Vec<_> = calls
            .iter()
            .filter(|v| v["method"] == "turn/start")
            .collect();
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0]["params"]["clientUserMessageId"], prior.submission);
        assert_eq!(starts[0]["params"]["effort"], "high");
        let connection = calls
            .iter()
            .find(|v| {
                v["method"]
                    == if thread.is_some() {
                        "thread/resume"
                    } else {
                        "thread/start"
                    }
            })
            .unwrap();
        assert_eq!(connection["params"]["model"], "frozen-fixture-model");
        assert_eq!(connection["params"]["sandbox"], "read-only");
        assert_eq!(
            connection["params"]["cwd"],
            Path::new(&prior.settings.workspace)
                .canonicalize()
                .unwrap()
                .display()
                .to_string()
        );
        if let Some(thread) = thread {
            assert_eq!(task.thread_id.as_deref(), Some(thread));
        }
        assert!(!calls.iter().any(|v| v["method"] == "thread/read"));
        drop(db);
        drop(core);
        let reopened = reopen(root.path());
        let stable = settled(&reopened, &prior.id);
        assert_eq!(stable.state, DelegationState::Completed);
        assert_eq!(stable.result, task.result);
        assert_eq!(stable.links, task.links);
        assert_eq!(stable.reports.len(), 1);
        assert_eq!(stable.report_call, task.report_call);
        assert_eq!(
            methods(Path::new(&prior.settings.workspace))
                .iter()
                .filter(|v| v["method"] == "turn/start")
                .count(),
            1
        );
    }
}

#[test]
fn reopen_never_revives_cancelled_or_failed_pre_dispatch_work() {
    for (state, thread) in [
        (DelegationState::CancellationRequested, None),
        (
            DelegationState::CancellationRequested,
            Some("pre-turn-thread"),
        ),
        (DelegationState::Cancelled, None),
        (DelegationState::Cancelled, Some("pre-turn-thread")),
        (DelegationState::Failed, None),
        (DelegationState::Failed, Some("pre-turn-thread")),
    ] {
        let (root, prior, _) = accepted_before_dispatch(state, thread);
        let core = reopen(root.path());
        let task = settled(&core, &prior.id);
        assert_eq!(
            task.state,
            if state == DelegationState::CancellationRequested {
                DelegationState::Cancelled
            } else {
                state
            }
        );
        assert!(!task.dispatch_started && task.turn_id.is_none());
        assert!(methods(Path::new(&prior.settings.workspace)).is_empty());
        drop(core);
        let reopened = reopen(root.path());
        let task = settled(&reopened, &prior.id);
        assert!(!task.dispatch_started && task.turn_id.is_none());
        assert!(methods(Path::new(&prior.settings.workspace)).is_empty());
    }
}

#[test]
fn running_worker_shutdown_at_initialize_or_thread_barrier_preserves_first_dispatch() {
    for mode in ["barrier-initialize", "barrier-thread"] {
        for cancel in [false, true] {
            let (root, core, prior, _) = admitted_core(DelegationState::Queued, None);
            let workspace = Path::new(&prior.settings.workspace);
            std::fs::write(
                workspace.join("fake-options.json"),
                json!({"mode":mode}).to_string(),
            )
            .unwrap();
            core.codex.dispatch(&prior.id).unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            while !workspace.join("at-barrier").exists() {
                assert!(Instant::now() < until, "worker did not reach {mode}");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(!core.database.task(&prior.id).unwrap().dispatch_started);
            if cancel {
                core.cancel_delegation(&prior.id).unwrap();
            }
            let manager = Arc::downgrade(&core.codex);
            let closing = std::thread::spawn(move || drop(core));
            while !manager
                .upgrade()
                .is_some_and(|m| m.stopped.load(Ordering::Acquire))
            {
                assert!(Instant::now() < until, "shutdown did not start");
                std::thread::sleep(Duration::from_millis(5));
            }
            std::fs::write(workspace.join("release"), "release").unwrap();
            closing.join().unwrap();
            let db = rusqlite::Connection::open(root.path().join("data/app.sqlite")).unwrap();
            let record: String = db
                .query_row(
                    "SELECT record FROM delegated_tasks WHERE id=?1",
                    [&prior.id],
                    |r| r.get(0),
                )
                .unwrap();
            let paused: DelegatedTask = serde_json::from_str(&record).unwrap();
            assert_eq!(
                paused.state,
                if cancel {
                    DelegationState::Cancelled
                } else {
                    DelegationState::Queued
                }
            );
            assert_eq!(paused.submission, prior.submission);
            assert!(!paused.dispatch_started && paused.turn_id.is_none());
            assert_eq!(paused.thread_id.is_some(), mode == "barrier-thread");
            assert!(
                !methods(workspace)
                    .iter()
                    .any(|v| v["method"] == "turn/start")
            );
            drop(db);
            let reopened = reopen(root.path());
            let task = settled(&reopened, &prior.id);
            assert_eq!(
                task.state,
                if cancel {
                    DelegationState::Cancelled
                } else {
                    DelegationState::Completed
                }
            );
            assert_eq!(task.submission, prior.submission);
            assert_eq!(task.reports.len(), 1);
            assert_eq!(
                methods(workspace)
                    .iter()
                    .filter(|v| v["method"] == "turn/start")
                    .count(),
                usize::from(!cancel)
            );
            if !cancel && mode == "barrier-thread" {
                assert_eq!(task.thread_id, paused.thread_id);
                assert_eq!(
                    methods(workspace)
                        .iter()
                        .filter(|v| v["method"] == "thread/resume")
                        .count(),
                    1
                );
            }
            drop(reopened);
            let stable = reopen(root.path());
            assert_eq!(settled(&stable, &prior.id).reports.len(), 1);
            assert_eq!(
                methods(workspace)
                    .iter()
                    .filter(|v| v["method"] == "turn/start")
                    .count(),
                usize::from(!cancel)
            );
        }
    }
}
