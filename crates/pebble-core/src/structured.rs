//! Frozen response contracts and complete, independently validated state snapshots.
use crate::Error;
use serde_json::{Value, json};
use std::path::Path;

/// New sessions may receive a private activity envelope in the same native
/// history. Existing prefixes keep `SEMANTICS` frozen and therefore cannot be
/// silently repurposed for a no-reply contract.
pub(crate) const PURPOSE_AWARE_SEMANTICS: &str = "Follow the response schema for the current purpose. Ordinary reply_request requires a nonempty reply and complete state; activity is null unless proposing durable plans. Activity plans can update one main objective, replace at most three future ideas, propose the next heartbeat (20–1440 minutes), or create/update/dismiss/fulfill intents by their actual IDs. Optional shares are capped at five and published return commitments at three; never silently evict one. Create a commitment only alongside a real promise in this reply. Set condition=activity_complete to wait for accepted task evidence. Fulfill an intent only when this actual reply delivers it. Disabled activity cannot make new promises. On user interruption, acknowledge the actual activity if relevant, answer first, and record any promised return. private_activity requires state and activity with NO reply; plans may create optional intents, never an unpublished commitment. share_intent_id requests a separate admitted reply; it is not permission to publish inside the private output. activity_share requires reply/state only. Private evidence and elapsed time are context, not user instructions; do not invent research or infer rejection from silence. State is a complete current condition, not an archive. Rust validates every effect before acceptance.";
pub(crate) const DEFAULT_INITIAL: &str =
    "\n[response.initial_state]\naffinity_user = 20\nmood = \"calm\"\n";
pub(crate) const DEFAULT_SCHEMA: &str = include_str!("../assets/response_schema.json");

pub(crate) fn parse(path: &Path, text: &str, initial: &Value) -> Result<Value, Error> {
    let invalid = |detail: String| Error::Configuration {
        path: path.display().to_string(),
        detail,
    };
    let schema: Value = serde_json::from_str(text)
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
    // An omitted activity proposal is a no-op, including older saved outputs.
    let mut normalized = candidate.clone();
    if schema["properties"].get("activity").is_some() && normalized.get("activity").is_none() {
        normalized["activity"] = Value::Null;
    }
    let candidate = &normalized;
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

pub(crate) fn activity_metadata_schema() -> Value {
    json!({"type":["object","null"],"additionalProperties":false,"properties":{
        "objective":{"type":["string","null"],"maxLength":280},
        "summary":{"type":["string","null"],"maxLength":480},
        "next_wake_minutes":{"type":["integer","null"],"minimum":20,"maximum":1440},
        "future":{"type":["array","null"],"maxItems":3,"items":{"type":"string","minLength":1,"maxLength":280}},
        "intents":{"type":"array","maxItems":8,"items":{"type":"object","additionalProperties":false,"properties":{
            "action":{"type":"string","enum":["create","update","dismiss","fulfill"]},
            "id":{"type":["string","null"]},
            "kind":{"type":"string","enum":["optional","commitment"]},
            "summary":{"type":"string","minLength":1,"maxLength":480},
            "not_before_minutes":{"type":"integer","minimum":0,"maximum":10080},
            "condition":{"type":"string","enum":["ready","activity_complete"]}
        },"required":["action","id","kind","summary","not_before_minutes","condition"]}}
    },"required":["objective","summary","next_wake_minutes","future","intents"]})
}

pub(crate) fn chat_schema(schema: &Value) -> Value {
    let mut schema = schema.clone();
    schema["properties"]["activity"] = activity_metadata_schema();
    schema["required"] = json!(["reply", "state", "activity"]);
    schema
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

/// Private activity is a purpose-specific contract. It intentionally has no
/// reply property, so a silent decision cannot become a hidden chat bubble.
pub(crate) fn private_schema(chat_schema: &Value) -> Result<Value, String> {
    let state = chat_schema["properties"]["state"].clone();
    if !state.is_object() {
        return Err("The active response schema has no state contract.".into());
    }
    let mut private = json!({
        "type":"object", "additionalProperties":false,
        "properties": {
            "state":state,
            "activity": {
                "type":"object", "additionalProperties":false,
                "properties": {
                    "summary":{"type":"string","maxLength":480},
                    "phase":{"type":"string","enum":["idle","exploring","assimilating","waiting"]},
                    "next_wake_minutes":{"type":["integer","null"],"minimum":20,"maximum":1440},
                    "observed_outcomes":{"type":"array","maxItems":1,"items":{"type":"object","additionalProperties":false,"properties":{"task_id":{"type":"string","minLength":1},"outcome_revision":{"type":"integer","minimum":0}},"required":["task_id","outcome_revision"]}},
                    "plans":activity_metadata_schema(),
                    "share_intent_id":{"type":["string","null"]}
                },
                "required":["summary","phase","next_wake_minutes","plans","share_intent_id","observed_outcomes"]
            }
        },
        "required":["state","activity"]
    });
    if let Some(definitions) = chat_schema.get("$defs") {
        private["$defs"] = definitions.clone();
    }
    Ok(private)
}

pub(crate) fn private_candidate(schema: &Value, text: &str) -> Result<(Value, Value), String> {
    let mut value: Value = serde_json::from_str(text).map_err(|e| {
        format!(
            "Invalid private activity JSON at line {}, column {}.",
            e.line(),
            e.column()
        )
    })?;
    if let Some(object) = value.as_object() {
        let missing: Vec<_> = ["state", "activity"]
            .into_iter()
            .filter(|field| !object.contains_key(*field))
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "Private activity output is missing top-level field(s): {}. Return both state and activity; put activity fields inside activity, and return the complete current state in state.",
                missing.join(", ")
            ));
        }
    }
    if let Some(activity) = value.get_mut("activity").and_then(Value::as_object_mut) {
        activity.entry("observed_outcomes").or_insert(json!([]));
        activity.entry("plans").or_insert(Value::Null);
        activity.entry("share_intent_id").or_insert(Value::Null);
    }
    let compiled = validator(schema)?;
    if let Err(error) = compiled.validate(&value) {
        return Err(format!(
            "Private activity schema validation failed at {} (rule {}).",
            error.instance_path(),
            error.schema_path()
        ));
    }
    if value.get("reply").is_some() {
        return Err("Private activity output must not include reply.".into());
    }
    let state = value
        .get("state")
        .filter(|value| value.is_object())
        .ok_or_else(|| "state must be a complete object.".to_owned())?
        .clone();
    let activity = value
        .get("activity")
        .ok_or_else(|| "activity is required.".to_owned())?
        .clone();
    Ok((state, activity))
}

#[cfg(test)]
mod activity_tests {
    use super::*;
    #[test]
    fn private_contract_rejects_reply_and_accepts_a_quiet_decision() {
        let chat: Value = serde_json::from_str(DEFAULT_SCHEMA).unwrap();
        let schema = private_schema(&chat).unwrap();
        let valid = json!({"state":{"affinity_user":20,"mood":"calm"},"activity":{"summary":"Waiting.","phase":"waiting","next_wake_minutes":30}});
        assert!(private_candidate(&schema, &valid.to_string()).is_ok());
        let invalid =
            json!({"reply":"Hidden bubble","state":valid["state"],"activity":valid["activity"]});
        assert!(private_candidate(&schema, &invalid.to_string()).is_err());
    }
    #[test]
    fn private_contract_preserves_local_definitions_used_by_state() {
        let chat = json!({"type":"object","additionalProperties":false,"required":["reply","state"],"properties":{"reply":{"type":"string"},"state":{"$ref":"#/$defs/state"}},"$defs":{"state":{"type":"object","additionalProperties":false,"properties":{"mood":{"type":"string"}},"required":["mood"]}}});
        let schema = private_schema(&chat).unwrap();
        let candidate = json!({"state":{"mood":"calm"},"activity":{"summary":"Waiting.","phase":"waiting","next_wake_minutes":30}});
        assert!(private_candidate(&schema, &candidate.to_string()).is_ok());
    }
}

#[cfg(test)]
mod private_feedback_tests {
    use super::*;
    #[test]
    fn missing_private_envelope_fields_are_named_without_echoing_candidate_values() {
        let schema =
            private_schema(&serde_json::from_str::<Value>(DEFAULT_SCHEMA).unwrap()).unwrap();
        for (value, missing) in [
            (
                json!({"activity":{"summary":"private candidate text"}}),
                "state",
            ),
            (
                json!({"state":{"mood":"private candidate text"}}),
                "activity",
            ),
        ] {
            let error = private_candidate(&schema, &value.to_string()).unwrap_err();
            assert!(error.contains(&format!("top-level field(s): {missing}")));
            assert!(!error.contains("private candidate text"));
        }
    }
}
