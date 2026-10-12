mod delegation_support;
mod live_support;
mod responses_support;
use delegation_support::{assert_fake, configure as codex_configure, settled, tool, wait};
use live_support::*;
use pebble_core::{DelegationState, TaskAnswer};
use serde_json::{Value, json};

fn answer(id: &str, values: &[&str]) -> TaskAnswer {
    TaskAnswer {
        question_id: id.into(),
        answers: values.iter().map(|v| (*v).into()).collect(),
    }
}

#[test]
fn native_question_persists_header_and_option_consequences_without_changing_free_text_answers() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "question")),
        stream(success("Started.")),
        stream(success("Saved answer.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "question");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &persona, "Choose the scope of synthetic work.");
    terminal(&events);
    let task = wait(&core, |t| t[0].request.is_some()).remove(0);
    let request = task.request.unwrap();
    let question = &request.questions[0];
    assert_eq!(question.header, "Scope and cost");
    assert_eq!(question.options, ["Small", "Large"]);
    assert_eq!(
        question.option_descriptions,
        [
            "Read one synthetic file; no modifications.",
            "Inspect all synthetic files; higher latency."
        ]
    );
    let record: String = rusqlite::Connection::open(root.path().join("data/app.sqlite"))
        .unwrap()
        .query_row(
            "SELECT record FROM delegated_tasks WHERE id=?1",
            [&task.id],
            |row| row.get(0),
        )
        .unwrap();
    let saved: pebble_core::DelegatedTask = serde_json::from_str(&record).unwrap();
    let persisted = &saved.request.as_ref().unwrap().questions[0];
    assert_eq!(persisted.header, question.header);
    assert_eq!(persisted.option_descriptions, question.option_descriptions);
    core.answer_delegation(
        &task.id,
        &request.id,
        None,
        &[answer("choice", &["Only README.md please"])],
    )
    .unwrap();
    assert_eq!(settled(&core).state, DelegationState::Completed);
    let response: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        response["result"],
        json!({"answers":{"choice":{"answers":["Only README.md please"]}}})
    );
}

#[test]
fn managed_network_host_and_protocol_are_reviewable_before_allow_once() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "network")),
        stream(success("Started.")),
        stream(success("Saved approved result.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "approval-network");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(
        &core,
        &persona,
        "Review a synthetic managed-network request.",
    );
    terminal(&events);
    let task = wait(&core, |t| t[0].request.is_some()).remove(0);
    let request = task.request.unwrap();
    assert!(request.approval);
    assert!(request.summary.contains("Network host: api.example.com"));
    assert!(request.summary.contains("Network protocol: https"));
    assert!(request.summary.contains("did not supply operation text"));
    core.answer_delegation(&task.id, &request.id, Some(true), &[])
        .unwrap();
    settled(&core);
    let response: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(response["result"], json!({"decision":"accept"}));
}

#[test]
fn mcp_constraint_captions_match_original_schema_rejection_and_valid_optional_secret_answers() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "constraints")),
        stream(success("Started.")),
        stream(success("Saved valid form.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "form-constrained");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &persona, "Answer the synthetic constrained form.");
    terminal(&events);
    let task = wait(&core, |t| t[0].request.is_some()).remove(0);
    let request = task.request.unwrap();
    let question = |id: &str| request.questions.iter().find(|q| q.id == id).unwrap();
    assert_eq!(
        question("age").description,
        "Eligible age for this synthetic request."
    );
    assert_eq!(
        question("age").guidance,
        ["Minimum: 18", "Maximum: 21", "Suggested default: 18"]
    );
    assert_eq!(
        question("amount").guidance,
        ["Minimum: 0.5", "Maximum: 1.5"]
    );
    assert_eq!(
        question("code").guidance,
        [
            "Minimum length (characters): 2",
            "Maximum length (characters): 4",
            "Suggested default: \"AB\""
        ]
    );
    assert_eq!(question("email").guidance, ["Format: email"]);
    assert_eq!(
        question("areas").guidance,
        ["Minimum selections: 1", "Maximum selections: 2"]
    );
    assert_eq!(question("flag").guidance, ["Suggested default: false"]);
    assert!(question("secret").secret && !question("secret").required);
    assert!(
        !serde_json::to_string(question("secret"))
            .unwrap()
            .contains("SECRET_DEFAULT")
    );
    let base = vec![
        answer("age", &["19"]),
        answer("areas", &["src"]),
        answer("code", &[""]),
        answer("secret", &[""]),
    ];
    for invalid in [
        answer("age", &["17"]),
        answer("age", &["22"]),
        answer("amount", &["0.1"]),
        answer("amount", &["2"]),
        answer("code", &["a"]),
        answer("code", &["abcde"]),
        answer("email", &["invalid"]),
        answer("secret", &["a"]),
        answer("secret", &["123456789"]),
        answer("areas", &[]),
        answer("areas", &["src", "docs", "src"]),
    ] {
        let mut answers = base.clone();
        answers.retain(|a| a.question_id != invalid.question_id);
        answers.push(invalid);
        assert!(
            core.answer_delegation(&task.id, &request.id, None, &answers)
                .is_err()
        );
        assert!(!workspace.path().join("fake-response.json").exists());
    }
    core.answer_delegation(&task.id, &request.id, None, &base)
        .unwrap();
    assert_eq!(settled(&core).state, DelegationState::Completed);
    let response: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        response["result"]["content"],
        json!({"age":19,"areas":["src"]})
    );
}

#[test]
fn arbitrary_pattern_constraint_is_explicitly_unsupported_without_an_answer() {
    let server = Server::memory(vec![
        stream(tool("chat_completions", "pattern")),
        stream(success("Started.")),
        stream(success("Unsupported constraint.")),
    ]);
    let (root, core, persona) = isolated();
    configure(root.path(), &server.url, "mock", "chat_completions", "");
    let workspace = tempfile::tempdir().unwrap();
    codex_configure(&core, workspace.path(), "form-pattern");
    assert_fake(&core, workspace.path());
    let (_handle, events) = start(&core, &persona, "Review a synthetic arbitrary form.");
    terminal(&events);
    let task = wait(&core, |t| t[0].detail.contains("form is unsupported")).remove(0);
    assert!(task.request.is_none());
    core.cancel_delegation(&task.id).unwrap();
    settled(&core);
    let response: Value = serde_json::from_slice(
        &std::fs::read(workspace.path().join("fake-response.json")).unwrap(),
    )
    .unwrap();
    assert!(response.get("error").is_some() && response.get("result").is_none());
}
