//! Concrete wire parameters. Configuration validation prevents any runtime-field collision.
use serde_json::{Map, Value};

/// Inspect TOML's original types before Serde can represent date/time values as
/// private JSON objects. JSON-compatible scalar values retain their authored types.
pub(crate) fn validate_toml(value: &toml::Value, field: &str) -> Result<(), String> {
    match value {
        toml::Value::Datetime(_) => Err(format!(
            "{field} is a TOML date/time value, which request parameters do not support. Quote intended date text as a string."
        )),
        toml::Value::Array(values) => values
            .iter()
            .enumerate()
            .try_for_each(|(index, value)| validate_toml(value, &format!("{field}[{index}]"))),
        toml::Value::Table(values) => values
            .iter()
            .try_for_each(|(key, value)| validate_toml(value, &format!("{field}.{key}"))),
        _ => Ok(()),
    }
}

fn protected(key: &str) -> bool {
    matches!(
        key,
        "model"
            | "input"
            | "messages"
            | "stream"
            | "stream_options"
            | "n_predict"
            | "max_new_tokens"
            | "max_tokens"
            | "max_completion_tokens"
            | "max_output_tokens"
            | "context_len"
            | "response_format"
            | "response_schema"
            | "json_schema"
            | "grammar"
            | "json_schema_strict"
            | "tools"
            | "tool_choice"
            | "parallel_tool_calls"
            | "functions"
            | "function_call"
            | "store"
            | "include"
            | "previous_response_id"
            | "conversation"
            | "instructions"
            | "truncation"
            | "background"
            | "n"
            | "best_of"
            | "echo"
            | "prompt"
            | "suffix"
            | "api"
            | "api_key"
            | "api_key_env"
            | "base_url"
            | "responses_schema_format"
            | "request"
            | "request_profile"
            | "extra_body"
            | "extra_headers"
            | "reasoning_format"
            | "parse_tool_calls"
            | "chat_template"
            | "response"
            | "response_contract"
            | "state"
            | "initial_state"
            | "memory_tools"
            | "memory_capable"
            | "return_tokens"
            | "return_prompt"
            | "apply_chat_template"
            | "add_generation_prompt"
            | "continue_final_message"
            | "tokenize"
            | "raw"
    )
}

pub(crate) fn validate(params: &Map<String, Value>, api: &str) -> Result<(), String> {
    for (key, value) in params {
        if protected(key) {
            return Err(format!(
                "request.{key} is controlled by Pebble; use its dedicated setting when available."
            ));
        }
        representable(value).map_err(|detail| format!("request.{key}: {detail}"))?;
        let valid = match key.as_str() {
            "temperature" => number(value, 0., 2.),
            "top_p" | "min_p" | "typical_p" | "tfs_z" => number(value, 0., 1.),
            "frequency_penalty" | "presence_penalty" => number(value, -2., 2.),
            "repeat_penalty" | "repetition_penalty" => value.as_f64().is_some_and(|v| v > 0.),
            "top_k" | "min_keep" => value.as_u64().is_some(),
            "seed" => value.as_i64().is_some(),
            "logprobs" => api == "chat_completions" && value.is_boolean(),
            "top_logprobs" => value.as_u64().is_some_and(|v| v <= 20),
            "stop" => {
                api == "chat_completions"
                    && (value.is_string()
                        || value
                            .as_array()
                            .is_some_and(|v| v.len() <= 4 && v.iter().all(Value::is_string)))
            }
            "logit_bias" => value.as_object().is_some_and(|v| {
                v.iter()
                    .all(|(k, v)| k.parse::<u64>().is_ok() && number(v, -100., 100.))
            }),
            "reasoning_effort" => api == "chat_completions" && effort(value),
            "reasoning" => api == "responses" && thinking::reasoning_valid(value),
            "text" => {
                api == "responses"
                    && value.as_object().is_some_and(|v| {
                        v.iter().all(|(k, v)| {
                            k == "verbosity"
                                && matches!(v.as_str(), Some("low" | "medium" | "high"))
                        })
                    })
            }
            "thinking" => thinking::thinking_valid(value),
            "enable_thinking" => value.is_boolean(),
            "thinking_budget" => value.as_u64().is_some(),
            "reasoning_budget" => value.as_i64().is_some_and(|v| v >= -1),
            "chat_template_kwargs" => value.as_object().is_some_and(|v| template(v).is_ok()),
            _ => true, // Explicit backend extensions are allowed, with unverified support shown in the UI.
        };
        if !valid {
            return Err(format!(
                "request.{key} has an invalid type, range, or protocol. Check the endpoint's parameter contract."
            ));
        }
    }
    if api == "chat_completions"
        && params.contains_key("top_logprobs")
        && params.get("logprobs") != Some(&Value::Bool(true))
    {
        return Err(
            "request.top_logprobs requires request.logprobs = true for Chat Completions.".into(),
        );
    }
    thinking::validate(params)?;
    Ok(())
}
fn number(v: &Value, min: f64, max: f64) -> bool {
    v.as_f64()
        .is_some_and(|v| v.is_finite() && (min..=max).contains(&v))
}
fn effort(v: &Value) -> bool {
    v.as_str().is_some_and(|v| !v.trim().is_empty())
}
fn template(v: &Map<String, Value>) -> Result<(), ()> {
    for (key, value) in v {
        if protected(key)
            || matches!(
                key.as_str(),
                "text" | "format" | "response_contract" | "state" | "memory_tools"
            )
            || (key == "enable_thinking" && !value.is_boolean())
            || (key == "reasoning_effort" && !effort(value))
            || (key == "reasoning" && !thinking::reasoning_valid(value))
            || (key == "thinking" && !thinking::thinking_valid(value))
            || (key == "thinking_budget" && value.as_u64().is_none())
            || (key == "reasoning_budget" && value.as_i64().is_none_or(|v| v < -1))
        {
            return Err(());
        }
        if let Some(nested) = value.as_object() {
            template(nested)?;
        }
    }
    Ok(())
}
fn representable(v: &Value) -> Result<(), &'static str> {
    match v {
        Value::Null => Err("null cannot be stored in TOML; omit the parameter instead"),
        Value::Number(v) if v.is_u64() && v.as_u64().unwrap() > i64::MAX as u64 => {
            Err("integer exceeds TOML's signed 64-bit range")
        }
        Value::Array(v) => v.iter().try_for_each(representable),
        Value::Object(v) => v.values().try_for_each(representable),
        _ => Ok(()),
    }
}

pub(crate) fn apply(params: &Map<String, Value>, body: &mut Value) {
    for (key, value) in params {
        // text.verbosity may share a container with Pebble's protected text.format.
        if key == "text" {
            for (key, value) in value.as_object().expect("validated text") {
                body["text"][key] = value.clone();
            }
        } else {
            body[key] = value.clone();
        }
    }
}

/// Template options other than the verified per-request thinking switch may alter
/// history formatting. Include their exact values in the existing durable binding.
pub(crate) fn template_identity(params: &Map<String, Value>) -> String {
    let mut template = params
        .get("chat_template_kwargs")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    template.remove("enable_thinking");
    if template.is_empty() {
        String::new()
    } else {
        format!("\nrequest_template={}", Value::Object(template))
    }
}

#[cfg(test)]
mod tests;
mod thinking;
