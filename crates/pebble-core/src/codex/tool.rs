use super::failure;
use crate::Error;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const INSTRUCTIONS: &str = "Use delegate_task for ALL current facts, external grounding, web/search, calendars, files, notes, research, and authorized actions/code work, even a trivial lookup. You have no direct work tools. Ask Codex a concrete task; it reuses local skills and available capabilities. Never invent searches, access, results, sources, or completed actions. It returns promptly with a stable task_id; do not poll or re-delegate. You can continue conversation while it works. A saved completion arrives separately; explain its actual evidence and limits in character. request_key is a short descriptive label only; it cannot create additional execution assignments. Rust assigns one shared task per user message by default, or one partition per explicitly mentioned character when the user selected separate tasks. Repeated same-partition requests reuse the saved task and reporting owner even if their instructions differ; ask for a separate user request to change the objective. title is a short user-facing title. instructions contains only relevant context, never unrelated private material in a group. task_id is null for new work; a supplied task_id continues your own task in its original room. For an uncertain task it only reads the saved outcome; execution is never resent. Execution uses a private task folder in Pebble application data and the configured sandbox policy. Questions and permission limitations are reported back through you after stopping the turn. Relay questions to the user in ordinary chat; continue the same task_id only after their actual reply. Never invent an answer, request secrets, or claim permission was granted. Image generation and editing use the separate generate_image and edit_image tools only; never delegate them.";
pub(crate) fn declaration(protocol: &str) -> Value {
    let parameters = json!({"type":"object","properties":{"title":{"type":"string"},"instructions":{"type":"string"},"request_key":{"type":"string"},"task_id":{"type":["string","null"]}},"required":["title","instructions","request_key","task_id"],"additionalProperties":false});
    if protocol == "responses" {
        json!({"type":"function","name":"delegate_task","description":INSTRUCTIONS,"parameters":parameters,"strict":true})
    } else {
        json!({"type":"function","function":{"name":"delegate_task","description":INSTRUCTIONS,"parameters":parameters,"strict":true}})
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    pub title: String,
    pub instructions: String,
    pub request_key: String,
    pub task_id: Option<String>,
}
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub id: String,
    pub arguments: Arguments,
}
#[derive(Debug)]
pub(crate) enum ParsedCall {
    None,
    Valid(Call),
    InvalidArguments { id: String, detail: String },
}

/// Establish an unambiguous native identity before allowing argument correction.
pub(crate) fn calls(output: &[Value]) -> Result<ParsedCall, Error> {
    let mut native = Vec::new();
    for item in output {
        if let Some(calls) = item.get("tool_calls") {
            for call in calls
                .as_array()
                .ok_or_else(|| failure("Invalid tool_calls array."))?
            {
                native.push((
                    &call["id"],
                    &call["function"]["name"],
                    &call["function"]["arguments"],
                    call["type"] == "function",
                ));
            }
        }
        if item["type"] == "function_call" {
            native.push((&item["call_id"], &item["name"], &item["arguments"], true));
        }
    }
    if !native
        .iter()
        .any(|(_, name, _, _)| **name == "delegate_task")
    {
        return Ok(ParsedCall::None);
    }
    if native.len() != 1 {
        return Err(failure(
            "Delegate exactly one task call; do not mix native calls.",
        ));
    }
    let (id, _, args, function) = native[0];
    if !function {
        return Err(failure("Invalid delegation tool type."));
    }
    let id = id
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 256)
        .ok_or_else(|| failure("Invalid delegation tool call ID."))?;
    let args = args
        .as_str()
        .filter(|s| s.len() <= 32768)
        .ok_or_else(|| failure("Delegation arguments exceed 32 KiB or are not a JSON string."))?;
    match arguments(args) {
        Ok(arguments) => Ok(ParsedCall::Valid(Call {
            id: id.into(),
            arguments,
        })),
        Err(detail) => Ok(ParsedCall::InvalidArguments {
            id: id.into(),
            detail,
        }),
    }
}

fn arguments(text: &str) -> Result<Arguments, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| {
        format!(
            "Arguments must be valid JSON (syntax error at line {}, column {}).",
            e.line(),
            e.column()
        )
    })?;
    let object = value
        .as_object()
        .ok_or("Arguments must be a JSON object.")?;
    let fields = ["title", "instructions", "request_key", "task_id"];
    let missing: Vec<_> = fields
        .iter()
        .filter(|key| !object.contains_key(**key))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(format!("Missing required fields: {}.", missing.join(", ")));
    }
    if object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err(
            "Extra fields are forbidden; use only title, instructions, request_key, task_id."
                .into(),
        );
    }
    for key in ["title", "instructions", "request_key"] {
        if !value[key].is_string() {
            return Err(format!("{key} must be a string."));
        }
    }
    if !value["task_id"].is_null() && !value["task_id"].is_string() {
        return Err("task_id must be explicitly null for new work, or a string containing an actual existing task ID for continuation.".into());
    }
    let arguments: Arguments = serde_json::from_str(text).map_err(|_| {
        "Each required field must occur exactly once with its declared type.".to_owned()
    })?;
    for (key, text, cap) in [
        ("title", arguments.title.as_str(), 160),
        ("instructions", arguments.instructions.as_str(), 16384),
        ("request_key", arguments.request_key.as_str(), 160),
    ] {
        if text.trim().is_empty() || text.len() > cap {
            return Err(format!(
                "{key} must be a nonempty string of at most {cap} UTF-8 bytes."
            ));
        }
    }
    Ok(arguments)
}

pub(crate) fn argument_feedback(protocol: &str, id: &str, detail: &str, remaining: usize) -> Value {
    crate::memory::result(protocol, id, &json!({
        "ok": false,
        "error": "invalid_arguments",
        "detail": detail,
        "corrective_retries_remaining": remaining,
        "instruction": "No task was accepted or executed. Correct the delegate_task call using exactly four required fields, with no extra keys or field names embedded inside values: title (nonempty string, at most 160 UTF-8 bytes), instructions (nonempty string, at most 16384 UTF-8 bytes), request_key (nonempty string, at most 160 UTF-8 bytes), task_id (explicit null for new work, or the actual existing task ID string for continuation). Do not invent a task ID or change the user's authorization. Return corrected JSON arguments, not prose."
    }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_validation_is_specific_bounded_and_requires_explicit_task_identity() {
        let valid = json!({"title":"Synthetic", "instructions":"Inspect synthetic evidence", "request_key":"shared", "task_id":null});
        assert!(arguments(&valid.to_string()).unwrap().task_id.is_none());
        for (field, bad, detail) in [
            ("title", json!(""), "title must be a nonempty"),
            ("instructions", json!(42), "instructions must be a string"),
            (
                "request_key",
                json!("x".repeat(161)),
                "request_key must be a nonempty",
            ),
            ("task_id", json!(false), "task_id must be explicitly null"),
        ] {
            let mut value = valid.clone();
            value[field] = bad;
            assert!(arguments(&value.to_string()).unwrap_err().contains(detail));
        }
        let mut absent = valid.clone();
        absent.as_object_mut().unwrap().remove("task_id");
        assert_eq!(
            arguments(&absent.to_string()).unwrap_err(),
            "Missing required fields: task_id."
        );
        let duplicate = valid
            .to_string()
            .replacen("{", "{\"title\":\"duplicate\",", 1);
        assert!(arguments(&duplicate).unwrap_err().contains("exactly once"));
        let mut extra = valid;
        extra["PRIVATE_UNKNOWN_FIELD"] = json!(true);
        let detail = arguments(&extra.to_string()).unwrap_err();
        assert!(detail.contains("Extra fields") && !detail.contains("PRIVATE"));
        let detail = arguments("{PRIVATE_BROKEN_JSON").unwrap_err();
        assert!(detail.contains("line 1, column") && !detail.contains("PRIVATE"));
    }
    #[test]
    fn native_identity_and_batch_errors_never_become_argument_corrections() {
        let call = json!({"type":"function_call","call_id":"synthetic","name":"delegate_task","arguments":"{}"});
        assert!(matches!(
            calls(std::slice::from_ref(&call)).unwrap(),
            ParsedCall::InvalidArguments { .. }
        ));
        assert!(calls(&[call.clone(), call.clone()]).is_err());
        for id in [Value::Null, json!(""), json!(" "), json!("x".repeat(257))] {
            let mut value = call.clone();
            value["call_id"] = id;
            assert!(calls(&[value]).is_err());
        }
        let mut oversized = call;
        oversized["arguments"] = json!("x".repeat(32769));
        assert!(calls(&[oversized]).is_err());
        assert!(calls(&[json!({"tool_calls":{}})]).is_err());
        assert!(calls(&[json!({"tool_calls":[{"type":"unsupported","id":"x","function":{"name":"delegate_task","arguments":"{}"}}]})]).is_err());
    }
}
