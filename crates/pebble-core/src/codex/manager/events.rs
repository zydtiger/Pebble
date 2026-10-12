use super::*;
use sha2::{Digest, Sha256};
pub(super) struct Change {
    pub terminal: bool,
    pub unsupported: Option<Value>,
}
/// Ignore every stale thread/turn callback; never display reasoning or raw tool logs.
pub(super) fn apply(
    task: &mut DelegatedTask,
    event: &Value,
    failure_origin: Option<DelegationState>,
) -> Result<Change, Error> {
    let mut change = Change {
        terminal: false,
        unsupported: None,
    };
    let method = event["method"].as_str().unwrap_or_default();
    let p = &event["params"];
    if p["threadId"].as_str() != task.thread_id.as_deref() {
        return Ok(change);
    }
    let turn = p["turnId"].as_str().or(p["turn"]["id"].as_str());
    if turn.is_some() && turn != task.turn_id.as_deref() {
        return Ok(change);
    }
    let failed_final = failure_origin.is_some()
        && task.state == DelegationState::Failed
        && ["item/completed", "turn/completed"].contains(&method)
        && turn.is_some()
        && event.get("id").is_none();
    if !task.state.active() && !failed_final {
        return Ok(change);
    }
    if let Some(id) = event.get("id") {
        if task.state == DelegationState::CancellationRequested {
            change.unsupported = Some(id.clone());
            return Ok(change);
        }
        let request_id = id.to_string();
        if task.resolved_requests.contains(&request_id)
            || task.request.as_ref().is_some_and(|r| r.id == request_id)
            || task.pending_requests.iter().any(|r| r.id == request_id)
        {
            return Ok(change);
        }
        if method == "item/tool/requestUserInput" {
            let questions = p["questions"]
                .as_array()
                .ok_or_else(|| failure("Codex question is malformed."))?;
            let questions = questions
                .iter()
                .map(|q| TaskQuestion {
                    id: q["id"].as_str().unwrap_or_default().into(),
                    required: true,
                    header: q["header"].as_str().unwrap_or_default().into(),
                    description: String::new(),
                    guidance: Vec::new(),
                    text: q["question"].as_str().unwrap_or_default().into(),
                    secret: q["isSecret"] == true,
                    choices: Vec::new(),
                    multiple: false,
                    options: q["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|o| o["label"].as_str().map(str::to_owned))
                        .collect(),
                    option_descriptions: q["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|o| o["label"].is_string())
                        .map(|o| o["description"].as_str().unwrap_or_default().into())
                        .collect(),
                })
                .collect();
            enqueue_request(
                task,
                TaskRequest {
                    id: id.to_string(),
                    approval: false,
                    summary: "Codex needs your answer".into(),
                    questions,
                    authorization_url: None,
                    method: method.into(),
                    payload: p.clone(),
                },
            );
        } else if [
            "item/commandExecution/requestApproval",
            "item/fileChange/requestApproval",
            "item/permissions/requestApproval",
        ]
        .contains(&method)
        {
            let reason = p["reason"]
                .as_str()
                .unwrap_or("Codex requests permission to proceed.");
            let action = if method.contains("fileChange") {
                "Write the proposed file changes".into()
            } else if method.contains("commandExecution") {
                requests::command_summary(p)
            } else {
                "Grant the requested permissions".into()
            };
            let cwd = p["cwd"].as_str().unwrap_or(&task.settings.workspace);
            let extra = if method == "item/permissions/requestApproval" {
                serde_json::to_string_pretty(&p["permissions"])?
            } else if method.contains("fileChange") {
                format!(
                    "Requested root: {}\n{}",
                    p["grantRoot"].as_str().unwrap_or("unchanged"),
                    task.approval_previews
                        .get(p["itemId"].as_str().unwrap_or_default())
                        .cloned()
                        .unwrap_or_else(
                            || "Codex did not supply a matching file-change preview.".into()
                        )
                )
            } else {
                String::new()
            };
            let summary = format!("{reason}\n{action}\nWorkspace: {cwd}\n{extra}");
            if summary.chars().count() > 8192 {
                change.unsupported = Some(id.clone());
                limitation(
                    task,
                    "Codex requested an approval whose full scope exceeds Pebble's native review limit. No permission was granted.",
                );
                return Ok(change);
            }
            enqueue_request(
                task,
                TaskRequest {
                    id: id.to_string(),
                    approval: true,
                    summary,
                    questions: Vec::new(),
                    authorization_url: None,
                    method: method.into(),
                    payload: p.clone(),
                },
            );
        } else if method == "mcpServer/elicitation/request" {
            let mode = p["mode"].as_str().unwrap_or_default();
            let questions = if mode == "url" {
                Vec::new()
            } else {
                let Some(questions) = requests::form_questions(&p["requestedSchema"]) else {
                    change.unsupported = Some(id.clone());
                    limitation(
                        task,
                        "This connected tool's form is unsupported; no answer or permission was sent.",
                    );
                    return Ok(change);
                };
                questions
            };
            let authorization_url = if mode == "url" {
                let value = p["url"].as_str().unwrap_or_default();
                if !url::Url::parse(value).is_ok_and(|u| ["https", "http"].contains(&u.scheme())) {
                    change.unsupported = Some(id.clone());
                    limitation(
                        task,
                        "This connected tool supplied an unsupported authorization URL; no answer or permission was sent.",
                    );
                    return Ok(change);
                }
                let digest = authorization_digest(value);
                if !task
                    .authorization_url_digests
                    .iter()
                    .any(|identity| identity.digest == digest)
                {
                    task.authorization_url_digests
                        .push(AuthorizationUrlIdentity {
                            digest,
                            bytes: value.len(),
                        });
                }
                task.result = redact_authorization(task, &task.result);
                task.detail = redact_authorization(task, &task.detail);
                task.local_limitation = redact_authorization(task, &task.local_limitation);
                task.links.retain(|link| {
                    !known_authorization(&task.authorization_url_digests, &link.target)
                });
                Some(value.to_owned())
            } else {
                None
            };
            enqueue_request(
                task,
                TaskRequest {
                    id: id.to_string(),
                    approval: mode == "url",
                    summary: format!(
                        "{}: {}{}",
                        p["serverName"].as_str().unwrap_or("Connected tool"),
                        p["message"].as_str().unwrap_or("Input requested"),
                        if mode == "url" {
                            " Open the authorization link, then explicitly confirm completion or deny."
                        } else {
                            ""
                        }
                    ),
                    questions,
                    authorization_url,
                    method: method.into(),
                    payload: p.clone(),
                },
            );
        } else {
            change.unsupported = Some(id.clone());
            limitation(
                task,
                "Codex requested an unsupported interaction. No permission was granted.",
            );
        }
        return Ok(change);
    }
    match method {
        "item/autoApprovalReview/started" | "item/autoApprovalReview/completed"
            if task.turn_id.is_some() && p["turnId"].as_str() == task.turn_id.as_deref() =>
        {
            // Codex owns the decision; a review outcome is never a turn outcome.
            if task.state == DelegationState::Running {
                task.activity = if method.ends_with("/started") {
                    "Codex is reviewing a permission request…"
                } else {
                    "Codex approval review finished; continuing…"
                }
                .into();
            }
            let outcome = match p["review"]["status"].as_str() {
                Some("denied") => Some("denied the requested action"),
                Some("timedOut") => Some("timed out before approving the requested action"),
                Some("aborted") => Some("stopped before approving the requested action"),
                _ => None,
            };
            if let Some(outcome) = outcome {
                let reason =
                    diagnostic(task, p["review"]["rationale"].as_str().unwrap_or_default());
                limitation(
                    task,
                    &format!("Codex automatic approval review {outcome}. {reason}"),
                );
            }
        }
        "error" if task.turn_id.is_some() && p["turnId"].as_str() == task.turn_id.as_deref() => {
            match p["willRetry"].as_bool() {
                Some(false) => {
                    task.state = DelegationState::Failed;
                    task.activity.clear();
                    task.request = None;
                    task.pending_requests.clear();
                    terminal_detail(
                        task,
                        "Codex reported a failed turn. Any completed effects remain in place.",
                        p,
                    );
                    change.terminal = true;
                }
                Some(true) => {
                    if task.state == DelegationState::Running {
                        task.activity = "Codex hit an error and is retrying…".into();
                    }
                    terminal_detail(task, "Codex reported an error and will retry this turn.", p);
                }
                None => {}
            }
        }
        "serverRequest/resolved" => {
            if let Some(id) = p.get("requestId") {
                resolve_request(task, &id.to_string());
            }
        }
        "item/started" => {
            if p["item"]["type"] == "fileChange" {
                let preview = p["item"]["changes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|change| {
                        format!(
                            "{}\n{}",
                            change["path"].as_str().unwrap_or("File"),
                            change["diff"].as_str().unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(id) = p["item"]["id"].as_str() {
                    task.approval_previews.insert(id.into(), preview);
                }
            }
            if task.state == DelegationState::Running {
                task.activity = match p["item"]["type"].as_str() {
                    Some("webSearch") => "Searching the web…",
                    Some("commandExecution") => "Running a task command…",
                    Some("fileChange") => "Updating workspace files…",
                    Some("mcpToolCall") => "Using a connected tool…",
                    _ => "Codex is working…",
                }
                .into();
            }
        }
        "item/completed" => capture_item(task, &p["item"]),
        "turn/completed" => {
            if failed_final {
                for item in p["turn"]["items"].as_array().into_iter().flatten() {
                    capture_item(task, item);
                }
            }
            task.request = None;
            task.pending_requests.clear();
            task.activity.clear();
            task.state = match p["turn"]["status"].as_str() {
                Some("completed") => DelegationState::Completed,
                Some("interrupted")
                    if task.state == DelegationState::CancellationRequested
                        || failure_origin == Some(DelegationState::CancellationRequested) =>
                {
                    DelegationState::Cancelled
                }
                Some("interrupted") => DelegationState::Interrupted,
                Some("failed") => DelegationState::Failed,
                _ => DelegationState::Uncertain,
            };
            preserve_deferred_outcome(task);
            let outcome = match task.state {
                DelegationState::Cancelled => {
                    "Codex confirmed interruption. Any completed effects remain in place."
                }
                DelegationState::Interrupted => {
                    "Codex interrupted this turn. Any completed effects remain in place."
                }
                DelegationState::Failed => {
                    "Codex reported a failed turn. Any completed effects remain in place."
                }
                DelegationState::Uncertain => {
                    "Codex returned an unrecognized outcome; no work will be resent."
                }
                _ => "",
            };
            // Some final frames omit the error already supplied by `error`.
            if !(failed_final
                && task.state == DelegationState::Failed
                && p["turn"]["error"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .is_empty())
            {
                terminal_detail(task, outcome, &p["turn"]);
            }
            change.terminal = true;
        }
        _ => {}
    }
    Ok(change)
}
fn capture_item(task: &mut DelegatedTask, item: &Value) {
    if task.state.active() {
        capture_summary(task, item);
    }
    if item["type"] == "webSearch"
        && let Some(url) = item["action"]["url"].as_str()
    {
        links(task, url);
    }
    if item["type"] == "agentMessage"
        && item["phase"] != "commentary"
        && let Some(text) = item["text"].as_str()
    {
        let safe_text = redact_authorization(task, text);
        links(task, &safe_text);
        task.result = clean_citations(&safe_text).chars().take(65536).collect();
        if task.result != text && text.contains('\u{e200}') {
            let failure_detail =
                (task.state == DelegationState::Failed).then(|| task.detail.clone());
            limitation(
                task,
                "Some Codex citation markers could not be resolved to URLs. Only actual returned source links are available; no sources were invented.",
            );
            if let Some(detail) = failure_detail {
                task.detail = if detail.contains(&task.local_limitation) {
                    detail
                } else {
                    format!("{detail}\n{}", task.local_limitation)
                };
            }
        }
    }
    if item["type"] == "fileChange" && item["status"] == "completed" {
        for changed in item["changes"].as_array().into_iter().flatten() {
            if let Some(path) = changed["path"].as_str() {
                let path = std::path::Path::new(path);
                let resolved = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    std::path::Path::new(&task.settings.workspace).join(path)
                };
                if let Ok(canonical) = resolved.canonicalize()
                    && task
                        .settings
                        .cwd()
                        .is_ok_and(|scope| canonical.starts_with(scope))
                {
                    push_link(
                        task,
                        TaskLink {
                            label: canonical
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into(),
                            target: canonical.display().to_string(),
                        },
                    );
                }
            }
        }
    }
}
fn preserve_deferred_outcome(task: &mut DelegatedTask) {
    if task.state == DelegationState::Completed && task.interaction_deferred {
        task.state = DelegationState::Failed;
    }
}
// Only the first short public commentary in this run describes the monitor entry.
fn capture_summary(task: &mut DelegatedTask, item: &Value) {
    if task.summary_captured || item["type"] != "agentMessage" || item["phase"] != "commentary" {
        return;
    }
    let Some(text) = item["text"].as_str() else {
        return;
    };
    let safe = clean_citations(&redact_authorization(task, text));
    let summary = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.is_empty() {
        return;
    }
    task.summary_captured = true;
    if summary.chars().count() > 160 || summary.contains("http") {
        return;
    }
    task.title = summary.chars().take(96).collect();
    task.summary_captured = true;
}

/// A hidden native request must never leave the background task waiting for UI input.
pub(super) fn defer_interaction(task: &mut DelegatedTask) -> Option<TaskRequest> {
    let request = task.request.take()?;
    task.interaction_deferred = true;
    let explanation = if request.method == "item/tool/requestUserInput"
        && !request.questions.iter().any(|q| q.secret)
    {
        let questions = request
            .questions
            .iter()
            .map(|q| {
                let options = q
                    .options
                    .iter()
                    .zip(&q.option_descriptions)
                    .map(|(label, description)| format!("{label}: {description}"))
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{} {} {}", q.header, q.text, options)
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Codex requested clarification. Pebble supplied no answer and requested interruption of this turn; no input remains pending in Pebble. Ask the user through the reporting character before continuing this same task: {questions}"
        )
    } else {
        "Pebble declined additional permission or connected-tool input and requested interruption of this turn. No permission or form answer was granted, and no approval remains pending in Pebble. Resolve the connection or execution scope in settings outside chat; do not send secrets or authorization tokens in conversation.".into()
    };
    let explanation = if explanation.chars().count() > 4096 {
        "Codex clarification exceeds the conversation relay limit. Pebble requested interruption of this turn; no input remains pending in Pebble. Ask for a shorter clarification before continuing this same task; no answer was supplied.".into()
    } else {
        explanation
    };
    limitation(task, &redact_authorization(task, &explanation));
    task.resolved_requests.push(request.id.clone());
    task.pending_requests.clear();
    task.state = DelegationState::Running;
    task.activity = "Pausing for the reporting character…".into();
    Some(request)
}
fn push_link(task: &mut DelegatedTask, link: TaskLink) {
    if !known_authorization(&task.authorization_url_digests, &link.target)
        && task.links.len() < 32
        && !task.links.iter().any(|l| l.target == link.target)
    {
        task.links.push(link);
    }
}
fn links(task: &mut DelegatedTask, text: &str) {
    let safe_text = redact_authorization(task, text);
    let text = safe_text.as_str();
    for target in source_urls(text) {
        if let Ok(url) = url::Url::parse(&target)
            && url.host_str().is_some()
        {
            push_link(
                task,
                TaskLink {
                    label: url.host_str().unwrap_or("Source").into(),
                    target,
                },
            );
        }
    }
    for token in text.split_whitespace() {
        let path_token = token
            .trim_matches(['`', '(', ')', '[', ']', ',', ';', '"'])
            .split_once("](")
            .map_or(
                token.trim_matches(['`', '(', ')', '[', ']', ',', ';', '"']),
                |(_, path)| path.trim_end_matches(')'),
            );
        if path_token.starts_with('/')
            && let Ok(path) = std::path::Path::new(path_token).canonicalize()
            && path.starts_with(&task.settings.workspace)
        {
            push_link(
                task,
                TaskLink {
                    label: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    target: path.display().to_string(),
                },
            );
        }
    }
}
pub(super) fn reconcile(task: &mut DelegatedTask, response: &Value) -> Result<(), Error> {
    let turns = response["thread"]["turns"]
        .as_array()
        .ok_or_else(|| failure("Saved Codex thread could not be read."))?;
    let found = turns
        .iter()
        .find(|t| t["id"].as_str() == task.turn_id.as_deref())
        .or_else(|| {
            if task.turn_id.is_none() {
                turns.iter().find(|t| {
                    t["items"].as_array().is_some_and(|items| {
                        items
                            .iter()
                            .any(|i| i["type"] == "userMessage" && i["clientId"] == task.submission)
                    })
                })
            } else {
                None
            }
        });
    let Some(turn) = found else {
        task.state = DelegationState::Uncertain;
        terminal_detail(
            task,
            "The saved thread does not confirm this submission. It will not be resent.",
            &Value::Null,
        );
        return Ok(());
    };
    task.turn_id = turn["id"].as_str().map(str::to_owned);
    for item in turn["items"].as_array().into_iter().flatten() {
        capture_summary(task, item);
        capture_item(task, item);
    }
    task.state = match turn["status"].as_str() {
        Some("completed") => DelegationState::Completed,
        Some("failed") => DelegationState::Failed,
        Some("interrupted") => DelegationState::Interrupted,
        _ => DelegationState::Uncertain,
    };
    preserve_deferred_outcome(task);
    let outcome = if task.state == DelegationState::Completed {
        "Recovered the completed result from the saved Codex thread."
    } else {
        "Reconciliation did not confirm completion. Follow up only after checking actual effects."
    };
    terminal_detail(task, outcome, turn);
    task.activity.clear();
    task.request = None;
    task.pending_requests.clear();
    Ok(())
}

fn limitation(task: &mut DelegatedTask, notice: &str) {
    if !task.local_limitation.is_empty() && !task.local_limitation.contains(notice) {
        task.local_limitation = format!("{}\n{notice}", task.local_limitation)
            .chars()
            .take(8192)
            .collect();
    } else if task.local_limitation.is_empty() {
        task.local_limitation = notice.into();
    }
    task.detail = task.local_limitation.clone();
}
fn terminal_detail(task: &mut DelegatedTask, outcome: &str, turn: &Value) {
    let error = diagnostic(task, turn["error"]["message"].as_str().unwrap_or_default());
    task.detail = redact_authorization(
        task,
        &[
            outcome.to_owned(),
            task.local_limitation.clone(),
            if error.is_empty() {
                String::new()
            } else {
                format!("Codex error: {error}")
            },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n"),
    );
}

pub(super) fn diagnostic(task: &DelegatedTask, text: &str) -> String {
    // Redact before bounding, so a long URL cannot leave a clipped credential prefix.
    redact_authorization(task, text)
        .lines()
        .take(4)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(1024)
        .collect()
}

fn authorization_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn known_authorization(identities: &[AuthorizationUrlIdentity], value: &str) -> bool {
    identities.iter().any(|identity| {
        identity.bytes == value.len() && identity.digest == authorization_digest(value)
    })
}
// Match only URLs actually received from this task's authorization requests. Digests
// survive resolution/restart and followups, but are never part of persona evidence.
pub(super) fn redact_authorization(task: &DelegatedTask, text: &str) -> String {
    if task.authorization_url_digests.is_empty() {
        return text.to_owned();
    }
    let mut safe = String::new();
    let lowercase = text.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(offset) = lowercase[cursor..]
        .find("https://")
        .into_iter()
        .chain(lowercase[cursor..].find("http://"))
        .min()
    {
        let start = cursor + offset;
        safe.push_str(&text[cursor..start]);
        let rest = &text[start..];
        let matched = task
            .authorization_url_digests
            .iter()
            .filter(|identity| {
                rest.get(..identity.bytes)
                    .is_some_and(|candidate| authorization_digest(candidate) == identity.digest)
            })
            .max_by_key(|identity| identity.bytes);
        if let Some(identity) = matched {
            safe.push_str("[authorization link removed]");
            cursor = start + identity.bytes;
        } else {
            // Advance past this scheme without modifying unrelated source URLs.
            safe.push_str(&rest[..7]);
            cursor = start + 7;
        }
    }
    safe.push_str(&text[cursor..]);
    safe
}

fn clean_citations(text: &str) -> String {
    let mut result = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('\u{e200}') {
        result.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(end) = tail.find('\u{e201}') {
            rest = &tail[end + '\u{e201}'.len_utf8()..];
        } else {
            rest = "";
            break;
        }
    }
    result.push_str(rest);
    result
}

fn source_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = text;
    while let Some(start) = rest
        .find("https://")
        .into_iter()
        .chain(rest.find("http://"))
        .min()
    {
        let delimited = rest[..start].ends_with('(') || rest[..start].ends_with('<');
        rest = &rest[start..];
        let mut parentheses = 0;
        let end = rest
            .char_indices()
            .find_map(|(index, c)| {
                if c == '(' {
                    parentheses += 1;
                    return None;
                }
                if c == ')' && parentheses > 0 {
                    parentheses -= 1;
                    return None;
                }
                (c.is_whitespace() || "<>\"'`)]{}".contains(c) || (!delimited && !c.is_ascii()))
                    .then_some(index)
            })
            .unwrap_or(rest.len());
        let target = rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if !target.is_empty() {
            urls.push(target.to_owned());
        }
        rest = &rest[end..];
    }
    urls
}
fn enqueue_request(task: &mut DelegatedTask, request: TaskRequest) {
    if task.request.is_none() {
        task.request = Some(request);
    } else {
        task.pending_requests.push(request);
    }
    present_request(task);
}
fn present_request(task: &mut DelegatedTask) {
    if task.state == DelegationState::CancellationRequested {
        return;
    }
    task.state = match task.request.as_ref() {
        Some(r) if r.approval => DelegationState::NeedsApproval,
        Some(_) => DelegationState::NeedsInput,
        None => DelegationState::Running,
    };
    task.activity = if task.request.is_none() {
        "Codex is working…".into()
    } else {
        String::new()
    };
}
pub(super) fn resolve_request(task: &mut DelegatedTask, id: &str) {
    if !task.resolved_requests.iter().any(|r| r == id) {
        task.resolved_requests.push(id.into());
    }
    task.pending_requests.retain(|r| r.id != id);
    if task.request.as_ref().is_some_and(|r| r.id == id) {
        task.request = if task.pending_requests.is_empty() {
            None
        } else {
            Some(task.pending_requests.remove(0))
        };
    }
    present_request(task);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_chinese_markdown_and_autolinks_keep_exact_source_urls() {
        let text = "[官网](https://rust-lang.org/)显示该版本，其链接指向[官方发布公告](https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/)，公告明确标注为，另见<https://example.org/a?q=1&b=2>。";
        assert_eq!(
            source_urls(text),
            vec![
                "https://rust-lang.org/",
                "https://blog.rust-lang.org/2026/10/01/Rust-1.99.0/",
                "https://example.org/a?q=1&b=2"
            ]
        );
        assert_eq!(
            source_urls(
                "[Page](https://example.org/wiki/Function_(math))与[中文](https://example.org/中文)"
            ),
            vec![
                "https://example.org/wiki/Function_(math)",
                "https://example.org/中文"
            ]
        );
        assert_eq!(
            clean_citations("事实\u{e200}cite\u{e202}turn0search0\u{e201}。"),
            "事实。"
        );
    }
}
