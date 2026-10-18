use crate::{ClassifierConnection, Error};
use serde_json::{Value, json};
use std::time::Duration;

fn failed(message: &str) -> Error {
    Error::Participation(message.into())
}
pub(crate) async fn classify(
    config: &ClassifierConnection,
    input: &str,
    phase: &str,
) -> Result<bool, Error> {
    let connection = &config.connection;
    let body = budgeted_body(config, input, phase)?;
    let route = if connection.api == "responses" {
        "responses"
    } else {
        "chat/completions"
    };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| failed("Could not initialize participation connection."))?;
    let mut request = client
        .post(format!(
            "{}/{route}",
            connection.base_url.trim_end_matches('/')
        ))
        .json(&body);
    if let Some(key) = crate::connection::credential(connection)? {
        request = request.bearer_auth(key);
    }
    let mut response=request.send().await.map_err(|_|failed("Participation connection failed or timed out; check its independent connection settings."))?;
    if !response.status().is_success() {
        return Err(failed(&format!(
            "Participation endpoint returned HTTP {}; check its connection and credentials.",
            response.status().as_u16()
        )));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failed("Participation response was interrupted."))?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(failed("Participation response exceeded the 1 MiB limit."));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| failed("Participation endpoint returned invalid JSON."))?;
    let text = if connection.api == "responses" {
        if value["status"] != "completed"
            || value.get("error").is_some_and(|v| !v.is_null())
            || value
                .get("incomplete_details")
                .is_some_and(|v| !v.is_null())
        {
            return Err(failed("Participation response was incomplete or refused."));
        }
        let output = value["output"]
            .as_array()
            .ok_or_else(|| failed("Participation output is missing."))?;
        if output.iter().any(|item| {
            !matches!(item["type"].as_str(), Some("message" | "reasoning"))
                || item.get("status").is_some_and(|v| v != "completed")
        }) {
            return Err(failed(
                "Participation output has an unsupported or incomplete item.",
            ));
        }
        if output
            .iter()
            .filter(|item| item["type"] == "message")
            .any(|item| {
                item["role"] != "assistant"
                    || item["status"] != "completed"
                    || item["content"].as_array().is_none_or(|parts| {
                        parts.iter().any(|part| {
                            !matches!(part["type"].as_str(), Some("output_text" | "refusal"))
                        })
                    })
            })
        {
            return Err(failed(
                "Participation message has an unsupported role, content, or status.",
            ));
        }
        crate::live::output_text(output)?
    } else {
        let choices = value["choices"]
            .as_array()
            .filter(|v| v.len() == 1)
            .ok_or_else(|| failed("Participation endpoint must return one choice."))?;
        let choice = &choices[0];
        let message = &choice["message"];
        if choice["finish_reason"] != "stop"
            || message["role"] != "assistant"
            || message
                .get("tool_calls")
                .is_some_and(|v| v.as_array().is_none_or(|v| !v.is_empty()))
            || message.get("function_call").is_some_and(|v| !v.is_null())
            || message["content"].as_array().is_some_and(|parts| {
                parts
                    .iter()
                    .any(|part| !matches!(part["type"].as_str(), Some("output_text" | "refusal")))
            })
        {
            return Err(failed("Participation output was truncated or unsupported."));
        }
        crate::live::output_text(std::slice::from_ref(message))?
    };
    super::phase_answer(&text, phase)
}

fn request_body(config: &ClassifierConnection, input: &Value, phase: &str) -> Result<Value, Error> {
    let connection = &config.connection;
    let prompt = match phase {
        "intent" => include_str!("../../assets/fallback-intent-prompt.txt").trim_end(),
        "permission" => include_str!("../../assets/fallback-permission-prompt.txt").trim_end(),
        _ => super::PROMPT.trim_end(),
    };
    let prompt = if connection.uses_json_object() {
        let key = if phase == "intent" {
            "whole_group_question"
        } else {
            "respond"
        };
        format!("{prompt}\nReturn only a JSON object, for example {{\"{key}\":true}}.")
    } else {
        prompt.to_owned()
    };
    let messages = vec![
        crate::storage::input_message(&connection.api, "system", &prompt),
        crate::images::input_message(&connection.api, input)?,
    ];
    let (_, mut body) = if connection.api == "responses" {
        (
            "responses",
            json!({"model":connection.model,"input":messages,"stream":false,"store":false,"max_output_tokens":connection.max_output_tokens}),
        )
    } else {
        (
            "chat/completions",
            json!({"model":connection.model,"messages":messages,"stream":false,"max_tokens":connection.max_output_tokens}),
        )
    };
    let schema = if phase == "intent" {
        json!({"type":"object","properties":{"whole_group_question":{"type":"boolean"}},"required":["whole_group_question"],"additionalProperties":false})
    } else {
        super::schema()
    };
    if connection.uses_json_object() {
        body["response_format"] = json!({"type":"json_object"});
    } else if connection.api == "chat_completions"
        || connection.responses_schema_format.as_deref() == Some("response_format")
    {
        body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"participation","strict":true,"schema":schema}});
    } else {
        body["text"]["format"] =
            json!({"type":"json_schema","name":"participation","strict":true,"schema":schema});
    }
    crate::request_parameters::apply(&connection.request, &mut body);
    Ok(body)
}

fn budgeted_body(config: &ClassifierConnection, input: &str, phase: &str) -> Result<Value, Error> {
    let mut input: Value = serde_json::from_str(input)?;
    let root = input
        .as_object_mut()
        .and_then(|v| v.remove("_asset_root"))
        .and_then(|v| v.as_str().map(std::path::PathBuf::from))
        .unwrap_or_default();
    let required = input
        .as_object_mut()
        .and_then(|v| v.remove("_required_message_ids"))
        .unwrap_or(json!([]));
    loop {
        let mut body = request_body(config, &input, phase)?;
        match config.connection.budget().check_body(&body) {
            Ok(()) => {
                let key = if config.connection.api == "responses" {
                    "input"
                } else {
                    "messages"
                };
                crate::images::materialize(&root, body[key].as_array_mut().unwrap())?;
                crate::images::check_transport(&body)?;
                return Ok(body);
            }
            Err(error) => {
                // Permission's real user transcript may contain an active silence
                // constraint: never silently remove it. Intent has only mandatory input.
                let mut removed = false;
                if !matches!(phase, "intent" | "permission") {
                    for key in ["recent_persona_events", "recent_group_transcript"] {
                        if let Some(events) = input[key].as_array_mut()
                            && let Some(index) = events.iter().position(|event| {
                                !required
                                    .as_array()
                                    .is_some_and(|ids| ids.contains(&event["message_id"]))
                            })
                        {
                            events.remove(index);
                            removed = true;
                            break;
                        }
                    }
                }
                if !removed {
                    return Err(failed(&format!(
                        "Participation mandatory input exceeds this classifier connection's budget before HTTP. Increase classifier context_len or reduce mandatory content. {error}"
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod budget_tests;
#[cfg(test)]
mod deepseek_tests;
