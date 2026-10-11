use crate::{Error, ModelRequest};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const MEMORY_CAP: usize = 8192;
// Six bytes per escaped ASCII control character, plus {"content":""}.
pub(crate) const ARGUMENT_CAP: usize = MEMORY_CAP * 6 + 14;
pub(crate) const NATIVE_CAP: usize = 131_072;
pub(crate) const INSTRUCTIONS: &str = "update_memory replaces your own MEMORY.md with a complete revised document. Preserve useful old memory, correct outdated facts, distinguish fictional and real-world context, and never duplicate current affinity/mood state. Call only when useful; unchanged content is a successful no-op. Never claim a save without a successful tool result. No other tools or file paths are allowed. Maximum MEMORY size is 8192 UTF-8 bytes.";

pub(crate) fn declaration(protocol: &str) -> Value {
    let parameters = json!({"type":"object","properties":{"content":{"type":"string","description":"Complete revised MEMORY.md, preserving useful existing knowledge."}},"required":["content"],"additionalProperties":false});
    if protocol == "responses" {
        json!({"type":"function","name":"update_memory","description":INSTRUCTIONS,"parameters":parameters,"strict":true})
    } else {
        json!({"type":"function","function":{"name":"update_memory","description":INSTRUCTIONS,"parameters":parameters,"strict":true}})
    }
}
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub id: String,
    pub arguments: String,
    pub content: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    content: String,
}

/// Validate the complete native batch before preparing any effect.
pub(crate) fn calls(output: &[Value]) -> Result<Vec<Call>, Error> {
    let mut calls = Vec::new();
    for item in output {
        if let Some(native) = item.get("tool_calls") {
            for call in native
                .as_array()
                .ok_or_else(|| invalid("Invalid tool_calls array."))?
            {
                if call["type"] != "function" {
                    return Err(invalid("Unsupported tool type."));
                }
                calls.push(parse(
                    &call["id"],
                    &call["function"]["name"],
                    &call["function"]["arguments"],
                )?);
            }
        }
        if item["type"] == "function_call" {
            calls.push(parse(&item["call_id"], &item["name"], &item["arguments"])?);
        }
    }
    if calls.len() > 1 {
        return Err(invalid(
            "Only one update_memory call is allowed per operation.",
        ));
    }
    Ok(calls)
}
fn parse(id: &Value, name: &Value, arguments: &Value) -> Result<Call, Error> {
    if name != "update_memory" {
        return Err(invalid("Unknown tool; only update_memory is supported."));
    }
    let id = id
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or_else(|| invalid("Invalid native tool call ID."))?;
    let arguments = arguments
        .as_str()
        .filter(|s| s.len() <= ARGUMENT_CAP)
        .ok_or_else(|| {
            invalid("Tool arguments exceed the 49166-byte bound or are not a JSON string.")
        })?;
    let parsed: Arguments = serde_json::from_str(arguments)
        .map_err(|_| invalid("update_memory requires exactly one string field: content."))?;
    if parsed.content.len() > MEMORY_CAP {
        return Err(invalid(
            "MEMORY exceeds 8192 UTF-8 bytes; consolidate it before saving.",
        ));
    }
    Ok(Call {
        id: id.into(),
        arguments: arguments.into(),
        content: parsed.content,
    })
}
pub(crate) fn result(protocol: &str, id: &str, result: &str) -> Value {
    if protocol == "responses" {
        json!({"type":"function_call_output","call_id":id,"output":result})
    } else {
        json!({"role":"tool","tool_call_id":id,"content":result})
    }
}
pub(crate) fn check_result(result: &Value) -> Result<(), Error> {
    let parsed: Value = result["output"]
        .as_str()
        .or(result["content"].as_str())
        .and_then(|s| serde_json::from_str(s).ok())
        .ok_or_else(|| invalid("Invalid durable tool result."))?;
    if parsed["ok"] != true {
        return Err(invalid(
            parsed["detail"].as_str().unwrap_or("Memory was not saved."),
        ));
    }
    Ok(())
}
pub(crate) fn append(request: &mut ModelRequest, items: &[Value]) -> Result<(), Error> {
    for item in items {
        request.items.push(crate::ModelItem {
            id: String::new(),
            sequence: i64::MAX,
            payload: serde_json::to_vec(item)?,
        });
    }
    Ok(())
}
pub(crate) fn invalid(detail: &str) -> Error {
    Error::Generation(format!("Memory tool failed: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worst_case_escaped_memory_fits_exact_argument_bound() {
        let escaped = |n| format!("{{\"content\":\"{}\"}}", "\\u0000".repeat(n));
        let accepted = escaped(MEMORY_CAP);
        assert_eq!(accepted.len(), ARGUMENT_CAP);
        let call = parse(&json!("bounded"), &json!("update_memory"), &json!(accepted)).unwrap();
        assert_eq!(call.content, "\0".repeat(MEMORY_CAP));
        assert!(
            parse(
                &json!("bounded"),
                &json!("update_memory"),
                &json!(escaped(MEMORY_CAP + 1))
            )
            .is_err()
        );
        let normal_oversize = json!({"content":"x".repeat(MEMORY_CAP + 1)}).to_string();
        assert!(
            parse(
                &json!("bounded"),
                &json!("update_memory"),
                &json!(normal_oversize)
            )
            .unwrap_err()
            .to_string()
            .contains("MEMORY exceeds")
        );
    }
}
