use crate::{ChatConnection, Error, ModelRequest, chat::Control, connection};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
mod chat_completions;
mod output_bound;
mod responses;
mod sse;

fn failure(detail: &str) -> Error {
    Error::Generation(detail.into())
}
fn network(error: reqwest::Error) -> Error {
    // Neither provider bodies nor URLs are copied into errors: they can contain secrets.
    failure(if error.is_timeout() {
        "The model request timed out."
    } else if error.is_connect() {
        "Could not connect to the model endpoint. Check the URL and network."
    } else {
        "The model connection failed."
    })
}

pub(crate) async fn generate(
    chat: &ChatConnection,
    request: &ModelRequest,
    output_tokens: u64,
    control: Arc<Control>,
    mut delta: impl FnMut(&str) -> Result<(), Error>,
) -> Result<Vec<Value>, Error> {
    let key = connection::credential(chat)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(network)?;
    let (route, body) = request_body(chat, request, output_tokens)?;
    chat.budget().check_body(&body)?;
    let url = format!("{}/{route}", chat.base_url.trim_end_matches('/'));
    let mut http = client.post(url).json(&body);
    if let Some(key) = key {
        http = http.bearer_auth(key);
    }
    let mut response = tokio::time::timeout(Duration::from_secs(45), http.send())
        .await
        .map_err(|_| failure("The model endpoint did not respond within 45 seconds."))?
        .map_err(network)?;
    if !response.status().is_success() {
        return Err(failure(&format!(
            "The model endpoint returned HTTP {}. Check connection and credentials.",
            response.status().as_u16()
        )));
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !mime
        .split(';')
        .next()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
    {
        return Err(failure("The model endpoint did not return an SSE stream."));
    }
    let mut framing = sse::Framing::default();
    let mut completion = chat_completions::Stream::default();
    completion.allow_tools =
        request.memory_tools || request.delegation_tools || request.image_generation_tools;
    completion.bound.enabled = request.memory_capable;
    let mut responses = responses::Stream::default();
    responses.allow_tools =
        request.memory_tools || request.delegation_tools || request.image_generation_tools;
    responses.bound.enabled = request.memory_capable;
    loop {
        let chunk = tokio::time::timeout(Duration::from_secs(45), response.chunk())
            .await
            .map_err(|_| failure("The model stream was idle for 45 seconds."))?
            .map_err(network)?;
        let Some(chunk) = chunk else {
            return Err(failure(
                "The model stream ended before its completion marker.",
            ));
        };
        if control.is_cancelled() {
            return Err(failure("Cancelled"));
        }
        let done = framing.feed(&chunk, &mut |data| {
            if chat.api == "responses" {
                responses.event(data, &mut delta)
            } else {
                completion.event(data, &mut delta)
            }
        })?;
        if done {
            return if chat.api == "responses" {
                responses.output()
            } else {
                completion.output().map(|item| vec![item])
            };
        }
    }
}

/// Extract only authoritative completed answer text, never opaque reasoning.
/// Refusal is a provider terminal outcome, not a candidate format error.
pub(crate) fn output_text(output: &[Value]) -> Result<String, Error> {
    let mut text = String::new();
    for item in output {
        if item["role"] == "assistant" {
            if item.get("refusal").is_some_and(|r| !r.is_null() && r != "") {
                return Err(failure(
                    "The model refused this reply; character state is unchanged.",
                ));
            }
            if item["content"].is_string() {
                text.push_str(item["content"].as_str().unwrap());
            } else if let Some(parts) = item["content"].as_array() {
                for part in parts {
                    if part["type"] == "refusal" {
                        return Err(failure(
                            "The model refused this reply; character state is unchanged.",
                        ));
                    }
                    if part["type"] == "output_text" {
                        text.push_str(part["text"].as_str().unwrap_or_default());
                    }
                }
            }
        }
    }
    Ok(text)
}

pub(crate) fn request_body(
    chat: &ChatConnection,
    request: &ModelRequest,
    output_tokens: u64,
) -> Result<(&'static str, Value), Error> {
    let mut messages = request
        .items
        .iter()
        .map(|item| serde_json::from_slice::<Value>(&item.payload))
        .collect::<Result<Vec<_>, _>>()?;
    crate::images::materialize(&request.image_root, &mut messages)?;
    let (route, mut body) = match chat.api.as_str() {
        "chat_completions" => (
            "chat/completions",
            json!({"model":request.model, "messages":messages, "stream":true, "max_tokens":output_tokens}),
        ),
        "responses" => (
            "responses",
            json!({"model":request.model, "input":messages, "stream":true, "store":false, "include":["reasoning.encrypted_content"], "max_output_tokens":output_tokens}),
        ),
        _ => return Err(failure("The configured model protocol is unsupported.")),
    };
    let replay_tools = messages.iter().any(|item| {
        item.get("tool_calls").is_some()
            || item["type"] == "function_call"
            || item["role"] == "tool"
            || item["type"] == "function_call_output"
    });
    if request.memory_tools
        || request.delegation_tools
        || request.image_generation_tools
        || replay_tools
    {
        let mut tools = vec![crate::memory::declaration(&request.protocol)];
        if request.delegation_tools
            || (request.response_schema.is_some()
                && messages.iter().any(|v| {
                    v["name"] == "delegate_task"
                        || v["tool_calls"].as_array().is_some_and(|calls| {
                            calls
                                .iter()
                                .any(|c| c["function"]["name"] == "delegate_task")
                        })
                }))
        {
            tools.push(crate::codex::tool::declaration(&request.protocol));
        }
        if request.image_generation_tools
            || messages.iter().any(|v| {
                v["name"] == "generate_image"
                    || v["tool_calls"].as_array().is_some_and(|c| {
                        c.iter().any(|v| v["function"]["name"] == "generate_image")
                    })
            })
        {
            let mut declaration = crate::image_generation::tool::declaration(&request.protocol);
            if request.image_generation_tools {
                let parameters = if request.protocol == "responses" {
                    &mut declaration["parameters"]
                } else {
                    &mut declaration["function"]["parameters"]
                };
                parameters["properties"]["backend"]["enum"] =
                    json!(request.image_generation_backends);
            }
            tools.push(declaration);
        }
        body["tools"] = json!(tools);
        body["parallel_tool_calls"] = json!(false);
        body["tool_choice"] = json!(if request.memory_tools
            || request.delegation_tools
            || request.image_generation_tools
        {
            if request.response_schema.is_none() {
                "required"
            } else {
                "auto"
            }
        } else {
            "none"
        });
    }
    if !request.memory_tools
        && !request.delegation_tools
        && !request.image_generation_tools
        && let Some(schema) = &request.response_schema
    {
        if chat.api == "chat_completions"
            || chat.responses_schema_format.as_deref() == Some("response_format")
        {
            body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"persona_response","strict":true,"schema":schema}});
        } else {
            body["text"]["format"] = json!({"type":"json_schema","name":"persona_response","strict":true,"schema":schema});
        }
    }
    crate::request_parameters::apply(&chat.request, &mut body);
    crate::images::check_transport(&body)?;
    Ok((route, body))
}

#[cfg(test)]
mod budget_tests;
