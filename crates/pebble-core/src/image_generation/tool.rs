use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub(crate) const INSTRUCTIONS: &str = "Use generate_image to create exactly one image through a configured backend. Use backend=studio by default. Use backend=chatgpt only when the user explicitly requests ChatGPT. If Studio is unavailable or fails, explain the limitation instead of switching backends automatically. Required arguments are prompt, backend (studio or chatgpt) and size {width,height} in pixels, with no extra fields. prompt is a JSON object: subject is a required nonblank description; style, composition, lighting, background and details are optional descriptive strings; constraints is an optional array of strings. Describe a self-contained image with concrete visual details, preserving the user's specifics. Use only applicable fields and set unused fields to null; do not invent gratuitous details. Values are freeform. Never put backend, model, path or pixel size in prompt. Pebble serializes the object directly as the backend prompt string, with a 16384 UTF-8 byte limit (Studio also limits it to 8000 characters). Never supply URLs, models, GPUs, paths or inference settings. Generation runs in the background and releases your conversation queue. An accepted result arrives separately with actual pixels for you to explain in the original room. Do not poll, repeat generation, delegate image generation or claim an image exists before receiving its saved result. Unsupported exact sizes fail before dispatch where possible; report any actual size mismatch honestly. New chat messages do not cancel background image work. There is no chat Stop control; do not promise cancellation through a message. Never use another backend as a silent fallback.";
pub(crate) const EDIT_INSTRUCTIONS: &str = "Use edit_image to edit exactly one published image from the CURRENT room that you can see in your frozen context. image_id is its exact asset ID, never a path or URL. If no source image is visible or the user's reference is ambiguous, ask the user to identify or attach the intended image before calling this tool. Never guess an image ID or silently choose among ambiguous sources. Return exactly one new image; the original stays unchanged. prompt.instruction is the required nonblank requested change; optional preserve and constraints are string arrays; optional style, composition, lighting, background and details are strings. Preserve the user's requested features, use only applicable fields, and set unused optional fields to null. Required arguments are image_id, prompt, backend and size {width,height}; size is always explicit. If the user did not specify a size, prefer the source original_size metadata, never the reduced vision rendition dimensions. If the original size is unknown, outside the supported 256–2048 range, or unsupported by the backend profile, explain the limitation and ask the user for a supported size; do not silently resize, invent dimensions or switch backends. Use studio by default, and chatgpt only when the user explicitly requests ChatGPT. Never fall back to another backend. The prompt object is serialized directly to compact JSON, capped at 16384 UTF-8 bytes and 8000 characters for Studio. No arbitrary paths, models, GPUs, inference settings, batches or variants. Editing shares the background lifecycle, result publication and cancellation rules of generate_image. Do not repeat, poll, delegate image editing or claim success before receiving saved pixels.";
pub(crate) fn declaration(protocol: &str) -> Value {
    named_declaration(protocol, false)
}
pub(crate) fn edit_declaration(protocol: &str) -> Value {
    named_declaration(protocol, true)
}
fn named_declaration(protocol: &str, edit: bool) -> Value {
    let mut p = json!({"type":"object","properties":{"prompt":super::prompt::schema(),"backend":{"type":"string","enum":["studio","chatgpt"]},"size":{"type":"object","properties":{"width":{"type":"integer","minimum":256,"maximum":2048},"height":{"type":"integer","minimum":256,"maximum":2048}},"required":["width","height"],"additionalProperties":false}},"required":["prompt","backend","size"],"additionalProperties":false});
    let (name, instructions) = if edit {
        p["properties"]["prompt"] = super::prompt::edit_schema();
        p["properties"]["image_id"] = json!({"type":"string","description":"Published source asset ID from this room's visible image attachments."});
        p["required"] = json!(["image_id", "prompt", "backend", "size"]);
        ("edit_image", EDIT_INSTRUCTIONS)
    } else {
        ("generate_image", INSTRUCTIONS)
    };
    if protocol == "responses" {
        json!({"type":"function","name":name,"description":instructions,"parameters":p,"strict":true})
    } else {
        json!({"type":"function","function":{"name":name,"description":instructions,"parameters":p,"strict":true}})
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeEditArguments {
    image_id: String,
    prompt: super::prompt::EditPrompt,
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
    if !calls
        .iter()
        .any(|(_, n, _)| **n == "generate_image" || **n == "edit_image")
    {
        return Ok(Parsed::None);
    }
    if calls.len() != 1 {
        return Err(super::failure("Use exactly one native tool call per turn."));
    }
    let (id, name, args) = calls[0];
    let id = id
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 256)
        .ok_or_else(|| super::failure("Invalid native image call ID."))?;
    let parsed = args
        .as_str()
        .filter(|s| s.len() <= 32768)
        .ok_or("Image arguments must be a bounded JSON string.")
        .and_then(|text| {
            if *name == "edit_image" {
                edit_arguments(text)
            } else {
                arguments(text)
            }
        });
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
        image_id: None,
        backend: a.backend,
        size: a.size,
    })
}
fn edit_arguments(text: &str) -> Result<Arguments, &'static str> {
    let a: NativeEditArguments = serde_json::from_str(text).map_err(|_| "Require image_id, prompt as an object with instruction and optional preserve/style/composition/lighting/background/details/constraints, backend and size {width,height}; reject extra or duplicate keys and incorrect types.")?;
    if uuid::Uuid::parse_str(&a.image_id).is_err() {
        return Err("image_id must be a published image asset ID from this room.");
    }
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
        prompt: a.prompt.serialize()?,
        image_id: Some(a.image_id),
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

#[cfg(test)]
mod edit_tests {
    use super::*;
    #[test]
    fn edit_instructions_require_source_clarification_and_original_size_intent() {
        for protocol in ["responses", "chat_completions"] {
            let tool = edit_declaration(protocol);
            let function = if protocol == "responses" {
                &tool
            } else {
                &tool["function"]
            };
            let description = function["description"].as_str().unwrap();
            assert_eq!(description, EDIT_INSTRUCTIONS);
            assert!(description.contains(
                "If no source image is visible or the user's reference is ambiguous, ask the user"
            ));
            assert!(description.contains("Never guess an image ID"));
            assert!(description.contains("prefer the source original_size metadata, never the reduced vision rendition dimensions"));
            assert!(
                description.contains("ask the user for a supported size; do not silently resize")
            );
            assert!(
                function["parameters"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("size"))
            );
        }
        let base = json!({"image_id":uuid::Uuid::new_v4().to_string(),"prompt":{"instruction":"make the cup blue"},"backend":"studio"});
        assert!(edit_arguments(&base.to_string()).is_err());
        for size in [
            json!({"width":1536,"height":1024}),
            json!({"width":2049,"height":1024}),
        ] {
            let mut args = base.clone();
            args["size"] = size.clone();
            assert_eq!(
                edit_arguments(&args.to_string()).is_ok(),
                size["width"] == 1536
            );
        }
    }
    #[test]
    fn strict_edit_schema_and_native_arguments_preserve_compact_payload() {
        let id = uuid::Uuid::new_v4().to_string();
        let valid = json!({"image_id":id,"prompt":{"instruction":"Change \"茶\"\ncolor", "preserve":["shape"],"style":null,"constraints":null},"backend":"studio","size":{"width":256,"height":512}}).to_string();
        let parsed = edit_arguments(&valid).unwrap();
        assert_eq!(parsed.image_id.as_deref(), Some(id.as_str()));
        assert_eq!(
            parsed.prompt,
            json!({"instruction":"Change \"茶\"\ncolor","preserve":["shape"]}).to_string()
        );
        for bad in [
            valid.replace("\"image_id\":", "\"extra\":0,\"image_id\":"),
            valid.replace(&id, "/tmp/source.png"),
            valid.replace(
                "\"instruction\":",
                "\"instruction\":\"duplicate\",\"instruction\":",
            ),
            valid.replace("\"style\":null", "\"style\":null,\"style\":null"),
            valid.replace("\"preserve\":[\"shape\"]", "\"preserve\":[1]"),
            valid.replace("\"width\":256", "\"width\":256.5"),
            valid.replace("\"height\":512", "\"height\":512,\"height\":512"),
        ] {
            assert!(edit_arguments(&bad).is_err(), "{bad}");
        }
        for prompt in [
            json!({}),
            json!({"instruction":" "}),
            json!({"instruction":null}),
            json!({"instruction":"x","subject":"wrong"}),
            json!({"instruction":"茶".repeat(6000)}),
        ] {
            let mut args: Value = serde_json::from_str(&valid).unwrap();
            args["prompt"] = prompt;
            assert!(edit_arguments(&args.to_string()).is_err());
        }
        for protocol in ["responses", "chat_completions"] {
            let tool = edit_declaration(protocol);
            let p = if protocol == "responses" {
                &tool["parameters"]
            } else {
                &tool["function"]["parameters"]
            };
            assert_eq!(
                p["required"],
                json!(["image_id", "prompt", "backend", "size"])
            );
            let p = &p["properties"]["prompt"];
            assert_eq!(p["additionalProperties"], false);
            assert_eq!(p["required"].as_array().unwrap().len(), 8);
            assert_eq!(
                p["properties"]["preserve"]["type"],
                json!(["array", "null"])
            );
            assert!(p["properties"].get("subject").is_none());
        }
        for output in [
            json!({"type":"function_call","call_id":"edit","name":"edit_image","arguments":valid}),
            json!({"role":"assistant","tool_calls":[{"id":"edit","function":{"name":"edit_image","arguments":valid}}]}),
        ] {
            assert!(matches!(calls(&[output]).unwrap(), Parsed::Valid(_)));
        }
    }
}
