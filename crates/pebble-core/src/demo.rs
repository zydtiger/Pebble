use crate::{Error, ModelRequest};
use serde_json::Value;

/// A deterministic local adapter that consumes the same stored request on every replay.
/// Its protocol is deliberately separate from any network provider's session.
pub(crate) fn reply(request: &ModelRequest) -> Result<String, Error> {
    let mut inputs = Vec::new();
    for item in &request.items {
        let payload: Value = serde_json::from_slice(&item.payload)?;
        if payload["role"] == "user" {
            let envelope: Value = serde_json::from_str(
                payload["content"]
                    .as_str()
                    .ok_or_else(|| Error::Generation("Invalid demo input envelope".into()))?,
            )?;
            for event in envelope["events"]
                .as_array()
                .ok_or_else(|| Error::Generation("Missing demo events".into()))?
            {
                if event["speaker"]["kind"] == "user" {
                    inputs.push(event["text"].as_str().unwrap_or_default().to_owned());
                }
            }
        }
    }
    let current = inputs.last().ok_or(Error::EmptyMessage)?;
    let previous = inputs.iter().rev().nth(1);
    let excerpt = |text: &str| text.chars().take(120).collect::<String>();
    if current
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    {
        let earlier = previous
            .map(|text| format!("\n\n上一条消息是：「{}」", excerpt(text)))
            .unwrap_or_default();
        Ok(format!(
            "本地演示 · 第 {} 次对话\n\n你说：「{}」{}\n\n聊天记录已保存在这台 Mac 上。此回复由本地演示程序生成，没有调用模型。",
            inputs.len(),
            excerpt(current),
            earlier
        ))
    } else {
        let earlier = previous
            .map(|text| format!("\n\nYour previous message was: “{}”", excerpt(text)))
            .unwrap_or_default();
        Ok(format!(
            "Local demo · Turn {}\n\nYou said: “{}”{}\n\nThis conversation is saved on this Mac. This reply comes from a local demo, with no model connected.",
            inputs.len(),
            excerpt(current),
            earlier
        ))
    }
}
