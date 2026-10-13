use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub(crate) const INSTRUCTIONS: &str = "Use generate_image to create exactly one image through a configured backend. Use backend=studio by default. Use backend=chatgpt only when the user explicitly requests ChatGPT. If Studio is unavailable or fails, explain the limitation instead of switching backends automatically. Required arguments are prompt, backend (studio or chatgpt) and size {width,height} in pixels, with no extra fields. prompt is a JSON object: subject is a required nonblank description; style, composition, lighting, background and details are optional descriptive strings; constraints is an optional array of strings. Describe a self-contained image with concrete visual details, preserving the user's specifics. Use only applicable fields and set unused fields to null; do not invent gratuitous details. Values are freeform. Never put backend, model, path or pixel size in prompt. Pebble serializes the object directly as the backend prompt string, with a 16384 UTF-8 byte limit (Studio also limits it to 8000 characters). Never supply URLs, models, GPUs, paths or inference settings. Generation runs in the background and releases your conversation queue. An accepted result arrives separately with actual pixels for you to explain in the original room. Do not poll, repeat generation, delegate image generation or claim an image exists before receiving its saved result. Unsupported exact sizes fail before dispatch where possible; report any actual size mismatch honestly. Stop cancels unpublished image work in this room. Never use another backend as a silent fallback.";
pub(crate) fn declaration(protocol: &str) -> Value {
    let p = json!({"type":"object","properties":{"prompt":super::prompt::schema(),"backend":{"type":"string","enum":["studio","chatgpt"]},"size":{"type":"object","properties":{"width":{"type":"integer","minimum":256,"maximum":2048},"height":{"type":"integer","minimum":256,"maximum":2048}},"required":["width","height"],"additionalProperties":false}},"required":["prompt","backend","size"],"additionalProperties":false});
    if protocol == "responses" {
        json!({"type":"function","name":"generate_image","description":INSTRUCTIONS,"parameters":p,"strict":true})
    } else {
        json!({"type":"function","function":{"name":"generate_image","description":INSTRUCTIONS,"parameters":p,"strict":true}})
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Size {
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Arguments {
    // Frozen backend payload. Legacy operations retain their original prose string.
    pub prompt: String,
    pub backend: String,
    pub size: Size,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeArguments {
    prompt: super::prompt::Prompt,
    backend: String,
    size: Size,
}
pub(crate) struct Call {
    pub id: String,
    pub arguments: Arguments,
}
pub(crate) enum Parsed {
    None,
    Valid(Call),
    Invalid { id: String, detail: String },
}
pub(crate) fn calls(output: &[Value]) -> Result<Parsed, Error> {
    let mut calls = Vec::new();
    for v in output {
        if let Some(values) = v["tool_calls"].as_array() {
            for c in values {
                calls.push((
                    &c["id"],
                    &c["function"]["name"],
                    &c["function"]["arguments"],
                ));
            }
        }
        if v["type"] == "function_call" {
            calls.push((&v["call_id"], &v["name"], &v["arguments"]));
        }
    }
    if !calls.iter().any(|(_, n, _)| **n == "generate_image") {
        return Ok(Parsed::None);
    }
    if calls.len() != 1 {
        return Err(super::failure("Use exactly one native tool call per turn."));
    }
    let (id, _, args) = calls[0];
    let id = id
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 256)
        .ok_or_else(|| super::failure("Invalid native image call ID."))?;
    let parsed = args
        .as_str()
        .filter(|s| s.len() <= 32768)
        .ok_or("Image arguments must be a bounded JSON string.")
        .and_then(arguments);
    Ok(match parsed {
        Ok(arguments) => Parsed::Valid(Call {
            id: id.into(),
            arguments,
        }),
        Err(detail) => Parsed::Invalid {
            id: id.into(),
            detail: detail.into(),
        },
    })
}
fn arguments(text: &str) -> Result<Arguments, &'static str> {
    let a: NativeArguments = serde_json::from_str(text).map_err(|_| "Require prompt as an object with subject and optional style, composition, lighting, background, details and constraints; backend and size {width,height}. Reject extra or duplicate keys and incorrect types.")?;
    let prompt = a.prompt.serialize()?;
    if !["studio", "chatgpt"].contains(&a.backend.as_str()) {
        return Err("backend must be studio or chatgpt.");
    }
    if [a.size.width, a.size.height]
        .iter()
        .any(|v| !(256..=2048).contains(v))
    {
        return Err("Each size dimension must be an integer from 256 to 2048.");
    }
    Ok(Arguments {
        prompt,
        backend: a.backend,
        size: a.size,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_contract_rejects_missing_extra_duplicate_and_invalid_dimensions() {
        let valid = r#"{"prompt":{"subject":"one pebble"},"backend":"studio","size":{"width":1024,"height":768}}"#;
        assert!(arguments(valid).is_ok());
        for bad in [
            valid.replace("1024", "1"),
            valid.replace("768", "768.5"),
            valid.replace("\"height\":768", "\"height\":768,\"seed\":1"),
            valid.replace("\"height\":768", "\"height\":768,\"height\":512"),
            valid.replace("\"studio\"", "\"other\""),
            valid.replace("\"backend\":", "\"backend\":\"studio\",\"backend\":"),
            valid.replace("\"prompt\":{\"subject\":\"one pebble\"},", ""),
            valid.replace("\"prompt\":", "\"prompt\":\"duplicate\",\"prompt\":"),
            valid.replace("{\"subject\":\"one pebble\"}", "\"legacy prose\""),
        ] {
            assert!(arguments(&bad).is_err(), "{bad}");
        }
        for api in ["responses", "chat_completions"] {
            let tool = declaration(api);
            let parameters = if api == "responses" {
                &tool["parameters"]
            } else {
                &tool["function"]["parameters"]
            };
            let prompt = &parameters["properties"]["prompt"];
            assert_eq!(prompt["type"], "object");
            assert_eq!(prompt["additionalProperties"], false);
            let required = prompt["required"].as_array().unwrap();
            let properties = prompt["properties"].as_object().unwrap();
            assert_eq!(required.len(), 7);
            assert_eq!(properties.len(), required.len());
            assert!(properties.keys().all(|key| required.contains(&json!(key))));
            assert_eq!(properties["subject"]["type"], "string");
            assert_eq!(properties["style"]["type"], json!(["string", "null"]));
            assert_eq!(properties["constraints"]["type"], json!(["array", "null"]));
            assert_eq!(properties["constraints"]["items"]["type"], "string");
        }
    }

    #[test]
    fn both_protocols_repair_plain_string_prompt_with_matched_native_error() {
        let args =
            json!({"prompt":"legacy prose","backend":"studio","size":{"width":256,"height":256}})
                .to_string();
        for output in [
            json!({"type":"function_call","call_id":"image-call","name":"generate_image","arguments":args}),
            json!({"role":"assistant","tool_calls":[{"id":"image-call","type":"function","function":{"name":"generate_image","arguments":args}}]}),
        ] {
            match calls(&[output]).unwrap() {
                Parsed::Invalid { id, detail } => {
                    assert_eq!(id, "image-call");
                    assert!(detail.contains("prompt as an object"));
                }
                _ => panic!("Plain string prompts must enter native argument repair"),
            }
        }
    }
}
