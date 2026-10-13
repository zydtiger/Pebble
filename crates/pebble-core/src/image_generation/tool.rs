use crate::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
pub(crate) const INSTRUCTIONS: &str = "Use generate_image to create exactly one image through a configured backend. Use backend=studio by default. Use backend=chatgpt only when the user explicitly requests ChatGPT. If Studio is unavailable or fails, explain the limitation instead of switching backends automatically. Required arguments are prompt, backend (studio or chatgpt) and size {width,height} in pixels, with no extra fields. Never supply URLs, models, GPUs, paths or inference settings. Generation runs in the background and releases your conversation queue. An accepted result arrives separately with actual pixels for you to explain in the original room. Do not poll, repeat generation, delegate image generation or claim an image exists before receiving its saved result. Unsupported exact sizes fail before dispatch where possible; report any actual size mismatch honestly. Stop cancels unpublished image work in this room. Never use another backend as a silent fallback.";
pub(crate) fn declaration(protocol: &str) -> Value {
    let p = json!({"type":"object","properties":{"prompt":{"type":"string"},"backend":{"type":"string","enum":["studio","chatgpt"]},"size":{"type":"object","properties":{"width":{"type":"integer","minimum":256,"maximum":2048},"height":{"type":"integer","minimum":256,"maximum":2048}},"required":["width","height"],"additionalProperties":false}},"required":["prompt","backend","size"],"additionalProperties":false});
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
    pub prompt: String,
    pub backend: String,
    pub size: Size,
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
    let a: Arguments = serde_json::from_str(text).map_err(|_| "Require exactly prompt, backend and size {width,height}, without extra or duplicate keys.")?;
    if a.prompt.trim().is_empty() || a.prompt.len() > 16384 {
        return Err("prompt must contain 1–16384 UTF-8 bytes.");
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
    Ok(a)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_contract_rejects_missing_extra_duplicate_and_invalid_dimensions() {
        let valid =
            r#"{"prompt":"one pebble","backend":"studio","size":{"width":1024,"height":768}}"#;
        assert!(arguments(valid).is_ok());
        for bad in [
            valid.replace("1024", "1"),
            valid.replace("768", "768.5"),
            valid.replace("\"height\":768", "\"height\":768,\"seed\":1"),
            valid.replace("\"studio\"", "\"other\""),
            valid.replace("\"prompt\":\"one pebble\",", ""),
            valid.replace("\"prompt\":", "\"prompt\":\"duplicate\",\"prompt\":"),
        ] {
            assert!(arguments(&bad).is_err(), "{bad}");
        }
        for api in ["responses", "chat_completions"] {
            assert!(declaration(api).to_string().contains("generate_image"));
        }
    }
}
