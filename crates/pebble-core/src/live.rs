use crate::{ChatConnection, Error, ModelRequest, chat::Control, connection};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
mod chat_completions;
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
    let messages = request
        .items
        .iter()
        .map(|item| serde_json::from_slice::<Value>(&item.payload))
        .collect::<Result<Vec<_>, _>>()?;
    let (route, body) = match chat.api.as_str() {
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
    let mut responses = responses::Stream::default();
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
