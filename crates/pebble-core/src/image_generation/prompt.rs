use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(super) fn schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "subject":{"type":"string","description":"Required nonblank visual subject, self-contained and faithful to the user's specifics."},
            "style":{"type":["string","null"]},
            "composition":{"type":["string","null"]},
            "lighting":{"type":["string","null"]},
            "background":{"type":["string","null"]},
            "details":{"type":["string","null"]},
            "constraints":{"type":["array","null"],"items":{"type":"string"}}
        },
        "required":["subject","style","composition","lighting","background","details","constraints"],
        "additionalProperties":false
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prompt {
    subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    composition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lighting: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    constraints: Option<Vec<String>>,
}

impl Prompt {
    pub(super) fn serialize(&self) -> Result<String, &'static str> {
        if self.subject.trim().is_empty() {
            return Err("prompt.subject must be a nonblank string.");
        }
        let payload = serde_json::to_string(self).map_err(|_| "Could not serialize prompt.")?;
        if payload.len() > 16384 {
            return Err("Serialized prompt must contain at most 16384 UTF-8 bytes.");
        }
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(text: &str) -> Result<String, String> {
        let prompt: Prompt = serde_json::from_str(text).map_err(|e| e.to_string())?;
        prompt.serialize().map_err(str::to_owned)
    }

    #[test]
    fn applicable_fields_serialize_directly_preserving_visual_details() {
        let prompt = json!({
            "subject":"一颗鹅卵石 beside tea",
            "style":"watercolor",
            "composition":"centered",
            "lighting":"soft morning light",
            "background":"white paper",
            "details":"label reads \"茶\"\nwith a green leaf",
            "constraints":["no border", "keep the label"]
        });
        assert_eq!(
            serde_json::from_str::<Value>(&payload(&prompt.to_string()).unwrap()).unwrap(),
            prompt
        );
        assert_eq!(payload(r#"{"subject":"one pebble","style":null,"composition":null,"lighting":null,"background":null,"details":null,"constraints":null}"#).unwrap(), payload(r#"{"subject":"one pebble"}"#).unwrap());
    }

    #[test]
    fn rejects_unknown_duplicate_missing_and_wrongly_typed_fields() {
        for text in [
            r#""legacy prose""#,
            r#"{}"#,
            r#"{"subject":null}"#,
            r#"{"subject":" \n\t"}"#,
            r#"{"subject":1}"#,
            r#"{"subject":"pebble","subject":"tea"}"#,
            r#"{"subject":"pebble","style":null,"style":"ink"}"#,
            r#"{"subject":"pebble","constraints":null,"constraints":[]}"#,
            r#"{"subject":"pebble","model":"qwen"}"#,
            r#"{"subject":"pebble","size":{"width":256}}"#,
            r#"{"subject":"pebble","style":[]}"#,
            r#"{"subject":"pebble","composition":{}}"#,
            r#"{"subject":"pebble","lighting":true}"#,
            r#"{"subject":"pebble","background":1}"#,
            r#"{"subject":"pebble","details":[]}"#,
            r#"{"subject":"pebble","constraints":"no text"}"#,
            r#"{"subject":"pebble","constraints":[null]}"#,
            r#"{"subject":"pebble","constraints":[{"text":"no text"}]}"#,
        ] {
            assert!(payload(text).is_err(), "{text}");
        }
    }

    #[test]
    fn byte_bound_includes_json_fields_escaping_and_multibyte_text() {
        let overhead = r#"{"subject":""}"#.len();
        let exact = json!({"subject":"x".repeat(16384 - overhead)}).to_string();
        assert_eq!(payload(&exact).unwrap().len(), 16384);
        for prompt in [
            json!({"subject":"x".repeat(16385 - overhead)}),
            json!({"subject":"茶".repeat(6000)}),
            json!({"subject":"\"".repeat(8192)}),
            json!({"subject":"pebble","constraints":["x".repeat(16384)]}),
        ] {
            assert!(payload(&prompt.to_string()).is_err());
        }
    }
}
