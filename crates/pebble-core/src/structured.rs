//! Frozen response contracts and complete, independently validated state snapshots.
use crate::{Error, file_error};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) const SEMANTICS: &str = "Return ONLY one JSON object with exactly reply and state, following RESPONSE_SCHEMA. reply is the nonempty user-facing text. state is the complete proposed next persona state derived from current_state and this turn; preserve unchanged values. State is the persona's current condition, not a memory archive. Follow schema descriptions and SOUL for meanings and updates; do not increase affinity for every ordinary message. No tools are available. Candidate state is not accepted until Pebble validates and commits it.";
pub(crate) const DEFAULT_INITIAL: &str =
    "\n[response.initial_state]\naffinity_user = 20\nmood = \"calm\"\n";
pub(crate) const DEFAULT_SCHEMA: &str = include_str!("../assets/response_schema.json");

pub(crate) fn load(path: &Path, initial: &Value) -> Result<Value, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| file_error(path, e))?;
    let invalid = |detail: String| Error::Configuration {
        path: path.display().to_string(),
        detail,
    };
    let schema: Value = serde_json::from_str(&text)
        .map_err(|_| invalid("response_schema.json must contain valid JSON.".into()))?;
    // Pebble owns the envelope. Authors customize the state properties and semantics.
    if schema["type"] != "object"
        || schema["additionalProperties"] != false
        || schema["properties"]
            .as_object()
            .is_none_or(|p| p.len() != 2)
        || schema["properties"]["reply"]["type"] != "string"
        || schema["properties"]["state"]["type"] != "object"
        || schema["properties"]["state"]["additionalProperties"] != false
        || schema["properties"]["state"]["properties"]
            .as_object()
            .is_none_or(|properties| {
                schema["properties"]["state"]["required"]
                    .as_array()
                    .is_none_or(|required| {
                        required.len() != properties.len()
                            || properties.keys().any(|key| !required.contains(&json!(key)))
                    })
            })
        || schema["required"].as_array().is_none_or(|r| {
            r.len() != 2 || !r.contains(&json!("reply")) || !r.contains(&json!("state"))
        })
    {
        return Err(invalid("The schema must require exactly reply (string) and state (object), with additionalProperties=false on both objects and every declared state field required.".into()));
    }
    validator(&schema).map_err(invalid)?;
    validate_state(&schema, initial).map_err(invalid)?;
    Ok(schema)
}

fn validator(schema: &Value) -> Result<jsonschema::Validator, String> {
    // HTTP/file reference resolution is disabled in Cargo features. Local $defs work.
    jsonschema::validator_for(schema).map_err(|e| format!("Invalid response schema: {e}"))
}

pub(crate) fn validate_state(schema: &Value, state: &Value) -> Result<(), String> {
    let mut state_schema = schema.clone();
    state_schema["properties"]["reply"] = json!({"type":"string"});
    validate(
        &state_schema,
        &json!({"reply":"State validation", "state":state}),
    )
    .map(|_| ())
}

pub(crate) fn validate(schema: &Value, candidate: &Value) -> Result<(String, Value), String> {
    let compiled = validator(schema)?;
    if let Err(error) = compiled.validate(candidate) {
        // Do not persist rejected reply/reasoning text in diagnostics or error bubbles.
        return Err(format!(
            "Schema validation failed at {} (rule {}).",
            error.instance_path(),
            error.schema_path()
        ));
    }
    let reply = candidate["reply"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "reply must be a nonempty string.".to_owned())?;
    if !candidate["state"].is_object() {
        return Err("state must be a complete object.".into());
    }
    Ok((reply.to_owned(), candidate["state"].clone()))
}

pub(crate) fn candidate(schema: &Value, text: &str) -> Result<(String, Value), String> {
    let value: Value = serde_json::from_str(text).map_err(|e| {
        format!(
            "Invalid response JSON at line {}, column {}.",
            e.line(),
            e.column()
        )
    })?;
    validate(schema, &value)
}
