use super::failure;
use crate::Error;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const INSTRUCTIONS: &str = "Use delegate_task for ALL current facts, external grounding, web/search, calendars, files, notes, research, and authorized actions/code work, even a trivial lookup. You have no direct work tools. Ask Codex a concrete task; it reuses local skills and available capabilities. Never invent searches, access, results, sources, or completed actions. It returns promptly with a stable task_id; do not poll or re-delegate. You can continue conversation while it works. A saved completion arrives separately; explain its actual evidence and limits in character. request_key is a short descriptive label only; it cannot create additional execution assignments. Rust assigns one shared task per user message by default, or one partition per explicitly mentioned character when the user selected separate tasks. Repeated same-partition requests reuse the saved task and reporting owner even if their instructions differ; ask for a separate user request to change the objective. title is a short user-facing title. instructions contains only relevant context, never unrelated private material in a group. task_id is null for new work; a supplied task_id continues your own task in its original room. Execution uses the user-selected app workspace/policy and may require native questions/approvals. Image generation is unavailable.";
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
pub(crate) fn calls(output: &[Value]) -> Result<Vec<Call>, Error> {
    let mut calls = Vec::new();
    for item in output {
        if let Some(native) = item["tool_calls"].as_array() {
            for call in native {
                if call["function"]["name"] == "delegate_task" {
                    calls.push(parse(&call["id"], &call["function"]["arguments"])?);
                }
            }
        }
        if item["type"] == "function_call" && item["name"] == "delegate_task" {
            calls.push(parse(&item["call_id"], &item["arguments"])?);
        }
    }
    let total = output
        .iter()
        .map(|item| {
            item["tool_calls"].as_array().map_or(0, Vec::len)
                + usize::from(item["type"] == "function_call")
        })
        .sum::<usize>();
    if !calls.is_empty() && total != 1 {
        return Err(failure(
            "Delegate exactly one task call; do not mix native calls.",
        ));
    }
    if calls.len() > 1 {
        return Err(failure(
            "Only one delegation is allowed per persona reply. Distinct requests can be sent separately.",
        ));
    }
    Ok(calls)
}
fn parse(id: &Value, args: &Value) -> Result<Call, Error> {
    let id = id
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or_else(|| failure("Invalid delegation tool call ID."))?;
    let args = args
        .as_str()
        .filter(|s| s.len() <= 32768)
        .ok_or_else(|| failure("Delegation arguments exceed 32 KiB."))?;
    let arguments: Arguments =
        serde_json::from_str(args).map_err(|_| failure("Invalid delegate_task arguments."))?;
    if arguments.title.trim().is_empty()
        || arguments.title.len() > 160
        || arguments.instructions.trim().is_empty()
        || arguments.instructions.len() > 16384
        || arguments.request_key.trim().is_empty()
        || arguments.request_key.len() > 160
    {
        return Err(failure(
            "Delegation needs a short title/request_key and instructions within 16 KiB.",
        ));
    }
    Ok(Call {
        id: id.into(),
        arguments,
    })
}
