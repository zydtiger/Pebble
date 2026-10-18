use super::failure;
use crate::Error;
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Stream {
    pub(super) bound: super::output_bound::Bound,
    content: String,
    reasoning: String,
    refusal: String,
    finish: Option<String>,
    tools: std::collections::BTreeMap<u64, Value>,
    pub(super) allow_tools: bool,
}
impl Stream {
    pub(super) fn event(
        &mut self,
        data: &str,
        delta: &mut impl FnMut(&str) -> Result<(), Error>,
    ) -> Result<bool, Error> {
        if data == "[DONE]" {
            return Ok(true);
        }
        let event: Value = serde_json::from_str(data)
            .map_err(|_| failure("The model stream contains malformed JSON."))?;
        if event.get("error").is_some() {
            return Err(failure("The model endpoint reported a streaming error."));
        }
        let choices = event["choices"]
            .as_array()
            .ok_or_else(|| failure("The model stream has no choices."))?;
        if choices.is_empty() && event.get("usage").is_some() {
            return Ok(false);
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
                    self.bound.add(text.len())?;
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
                "tool_calls" if self.allow_tools => {
                    for part in value
                        .as_array()
                        .ok_or_else(|| failure("Invalid native tool delta."))?
                    {
                        let index = part["index"]
                            .as_u64()
                            .filter(|i| *i == 0)
                            .ok_or_else(|| failure("Only one native tool call is supported."))?;
                        let call = self.tools.entry(index).or_insert_with(||json!({"id":"","type":"function","function":{"name":"","arguments":""}}));
                        for field in ["id", "type"] {
                            // Null marks a field with no update in this chunk.
                            if let Some(value) = part.get(field).filter(|value| !value.is_null()) {
                                if field == "type" && value != "function" {
                                    return Err(failure("Unsupported native tool type."));
                                }
                                if field == "id" {
                                    let text = value
                                        .as_str()
                                        .ok_or_else(|| failure("Invalid tool ID delta."))?;
                                    self.bound.add(text.len())?;
                                    call[field] = format!(
                                        "{}{}",
                                        call[field].as_str().unwrap_or_default(),
                                        text
                                    )
                                    .into();
                                }
                            }
                        }
                        for field in ["name", "arguments"] {
                            if let Some(value) =
                                part["function"].get(field).filter(|value| !value.is_null())
                            {
                                let text = value
                                    .as_str()
                                    .ok_or_else(|| failure("Invalid function delta."))?;
                                let length = call["function"][field]
                                    .as_str()
                                    .unwrap_or_default()
                                    .len()
                                    .saturating_add(text.len());
                                if length > crate::memory::ARGUMENT_CAP {
                                    return Err(failure(
                                        "Native tool arguments exceeded their bound.",
                                    ));
                                }
                                self.bound.add(text.len())?;
                                let text = format!(
                                    "{}{}",
                                    call["function"][field].as_str().unwrap_or_default(),
                                    text
                                );
                                call["function"][field] = text.into();
                            }
                        }
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
            if reason != "stop" && !(self.allow_tools && reason == "tool_calls") {
                return Err(failure(match reason {
                    "length" => "The model exhausted its output budget before completing a reply.",
                    "content_filter" => "The model filtered the reply.",
                    _ => "The model stopped with an unsupported finish reason.",
                }));
            }
        }
        Ok(false)
    }
    pub(super) fn output(self) -> Result<Value, Error> {
        if !self.refusal.is_empty() {
            return Err(failure(
                "The model refused this reply; character state is unchanged.",
            ));
        }
        if self.finish.as_deref() == Some("tool_calls")
            && self.allow_tools
            && !self.tools.is_empty()
        {
            let mut output = json!({"role":"assistant","content":if self.content.is_empty(){Value::Null}else{Value::String(self.content)},"tool_calls":self.tools.into_values().collect::<Vec<_>>()});
            if !self.reasoning.is_empty() {
                output["reasoning_content"] = self.reasoning.into();
            }
            return Ok(output);
        }
        if !self.tools.is_empty() {
            return Err(failure(
                "Tool calls require the native tool_calls finish reason.",
            ));
        }
        if self.finish.as_deref() != Some("stop") {
            return Err(failure("The model stream has no successful finish reason."));
        }
        if self.content.trim().is_empty() {
            if !self.refusal.is_empty() {
                return Err(failure(
                    "The model refused this reply; character state is unchanged.",
                ));
            }
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

    fn tool_event(parser: &mut Stream, part: Value) -> Result<bool, Error> {
        parser.event(
            &json!({"choices":[{"index":0,"delta":{"tool_calls":[part]},"finish_reason":null}]})
                .to_string(),
            &mut |_| Ok(()),
        )
    }

    #[test]
    fn null_tool_fields_preserve_strings_across_fragmented_ids_names_and_arguments() {
        let mut parser = Stream {
            allow_tools: true,
            ..Default::default()
        };
        for part in [
            json!({"index":0,"id":"call-","type":"function","function":{"name":"update_","arguments":null}}),
            json!({"index":0,"id":null,"type":null,"function":{"name":null,"arguments":"{"}}),
            json!({"index":0,"id":"1","function":{"name":"memory","arguments":"\"content\":\"test\"}"}}),
        ] {
            tool_event(&mut parser, part).unwrap();
        }
        parser
            .event(
                &json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})
                    .to_string(),
                &mut |_| Ok(()),
            )
            .unwrap();
        assert!(parser.event("[DONE]", &mut |_| Ok(())).unwrap());
        let output = parser.output().unwrap();
        assert_eq!(
            output["tool_calls"][0],
            json!({"id":"call-1","type":"function","function":{"name":"update_memory","arguments":"{\"content\":\"test\"}"}})
        );
        assert_eq!(crate::memory::calls(&[output]).unwrap()[0].content, "test");
    }

    #[test]
    fn non_null_invalid_tool_fields_and_missing_final_identity_still_fail() {
        for part in [
            json!({"index":0,"id":123}),
            json!({"index":0,"type":"other"}),
            json!({"index":0,"function":{"name":false}}),
            json!({"index":0,"function":{"arguments":{}}}),
        ] {
            let mut parser = Stream {
                allow_tools: true,
                ..Default::default()
            };
            assert!(tool_event(&mut parser, part).is_err());
        }
        let mut parser = Stream {
            allow_tools: true,
            ..Default::default()
        };
        tool_event(&mut parser, json!({"index":0,"id":null,"function":{"name":"update_memory","arguments":"{\"content\":\"test\"}"}})).unwrap();
        parser
            .event(
                &json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]})
                    .to_string(),
                &mut |_| Ok(()),
            )
            .unwrap();
        assert!(crate::memory::calls(&[parser.output().unwrap()]).is_err());
    }

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
        let mut framing = super::super::sse::Framing::default();
        let mut visible = String::new();
        let mut completed = false;
        for byte in source.bytes() {
            completed = framing
                .feed(&[byte], &mut |data| {
                    parser.event(data, &mut |delta| {
                        visible.push_str(delta);
                        Ok(())
                    })
                })
                .unwrap();
        }
        assert!(completed);
        assert_eq!(visible, "你好 🪨");
        assert_eq!(parser.output().unwrap()["reasoning_content"], "hidden");
    }
}
