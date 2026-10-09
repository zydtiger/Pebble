use crate::{ChatConnection, Error, ModelRequest, chat::Control, connection};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

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
    control: Arc<Control>,
    mut delta: impl FnMut(&str) -> Result<(), Error>,
) -> Result<Value, Error> {
    if chat.api != "chat_completions" {
        return Err(failure(
            "Responses is not implemented in this version. Choose chat_completions.",
        ));
    }
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
    let url = format!("{}/chat/completions", chat.base_url.trim_end_matches('/'));
    let mut http = client
        .post(url)
        .json(&json!({"model":request.model, "messages":messages,
        "stream":true, "max_tokens":4096}));
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
    let mut parser = Stream::default();
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
        if parser.feed(&chunk, &mut delta)? {
            return parser.output();
        }
    }
}

/// Parse complete SSE lines as bytes, so network fragments never split UTF-8 decoding.
#[derive(Default)]
struct Stream {
    pending: Vec<u8>,
    data: Vec<String>,
    frame_bytes: usize,
    content: String,
    reasoning: String,
    refusal: String,
    finish: Option<String>,
}
impl Stream {
    fn feed(
        &mut self,
        bytes: &[u8],
        delta: &mut impl FnMut(&str) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let mut line = self.pending.drain(..=end).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.frame_bytes += line.len();
            if self.frame_bytes > 1024 * 1024 {
                return Err(failure("The model SSE event is too large."));
            }
            let line = std::str::from_utf8(&line)
                .map_err(|_| failure("The model stream contains invalid UTF-8."))?;
            if line.is_empty() {
                let data = self.data.join("\n");
                self.data.clear();
                self.frame_bytes = 0;
                if data == "[DONE]" {
                    return Ok(true);
                }
                if !data.is_empty() {
                    self.event(&data, delta)?;
                }
            } else if let Some(value) = line.strip_prefix("data:") {
                self.data
                    .push(value.strip_prefix(' ').unwrap_or(value).into());
            }
        }
        if self.pending.len() > 1024 * 1024 {
            return Err(failure("The model SSE line is too large."));
        }
        Ok(false)
    }
    fn event(
        &mut self,
        data: &str,
        delta: &mut impl FnMut(&str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let event: Value = serde_json::from_str(data)
            .map_err(|_| failure("The model stream contains malformed JSON."))?;
        if event.get("error").is_some() {
            return Err(failure("The model endpoint reported a streaming error."));
        }
        let choices = event["choices"]
            .as_array()
            .ok_or_else(|| failure("The model stream has no choices."))?;
        if choices.is_empty() && event.get("usage").is_some() {
            return Ok(());
        }
        if choices.len() != 1 || choices[0]["index"] != 0 {
            return Err(failure("The model stream has an unexpected choice."));
        }
        let choice = &choices[0];
        let fields = choice["delta"]
            .as_object()
            .ok_or_else(|| failure("The model stream has no assistant delta."))?;
        if self.finish.is_some() && !fields.is_empty() {
            return Err(failure("The model emitted content after completion."));
        }
        for (field, value) in fields {
            if value.is_null() {
                continue;
            }
            match field.as_str() {
                "role" if value == "assistant" => {}
                "content" | "reasoning_content" | "refusal" => {
                    let text = value
                        .as_str()
                        .ok_or_else(|| failure("The model delta has an invalid text field."))?;
                    match field.as_str() {
                        "content" => {
                            self.content.push_str(text);
                            if !text.is_empty() {
                                delta(text)?;
                            }
                        }
                        "reasoning_content" => self.reasoning.push_str(text),
                        _ => self.refusal.push_str(text),
                    }
                }
                "tool_calls" | "function_call" => {
                    return Err(failure("The model requested unsupported tool execution."));
                }
                "role" => return Err(failure("The model delta has an unexpected role.")),
                _ => {}
            }
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            if self.finish.is_some() {
                return Err(failure("The model sent multiple finish reasons."));
            }
            self.finish = Some(reason.into());
            if reason != "stop" {
                return Err(failure(match reason {
                    "length" => "The model exhausted its output budget before completing a reply.",
                    "content_filter" => "The model filtered the reply.",
                    _ => "The model stopped with an unsupported finish reason.",
                }));
            }
        }
        Ok(())
    }
    fn output(self) -> Result<Value, Error> {
        if self.finish.as_deref() != Some("stop") {
            return Err(failure("The model stream has no successful finish reason."));
        }
        if self.content.trim().is_empty() {
            return Err(failure(
                "The model completed without visible reply content.",
            ));
        }
        let mut output = json!({"role":"assistant", "content":self.content});
        if !self.reasoning.is_empty() {
            output["reasoning_content"] = self.reasoning.into();
        }
        if !self.refusal.is_empty() {
            output["refusal"] = self.refusal.into();
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_fragments_multiline_events_and_comments_keep_utf8_and_reasoning_separate() {
        let source = concat!(
            ": heartbeat\r\n\r\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":\n",
            "data: {\"reasoning_content\":\"hidden\",\"content\":\"你好 🪨\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\n",
            "data: [DONE]\n\n"
        );
        let mut parser = Stream::default();
        let mut visible = String::new();
        let mut completed = false;
        for byte in source.bytes() {
            completed = parser
                .feed(&[byte], &mut |delta| {
                    visible.push_str(delta);
                    Ok(())
                })
                .unwrap();
        }
        assert!(completed);
        assert_eq!(visible, "你好 🪨");
        assert_eq!(parser.output().unwrap()["reasoning_content"], "hidden");
    }
}
