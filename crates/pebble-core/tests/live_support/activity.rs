//! Explicit synthetic output marker for task identities assigned at runtime.
use serde_json::{Value, json};

pub(super) fn fill_acknowledgments(request: &Value, wire: &mut String) {
    if !wire.contains("$observed_outcomes") {
        return;
    }
    let items = request["messages"]
        .as_array()
        .or_else(|| request["input"].as_array())
        .unwrap();
    let outcomes = items
        .iter()
        .rev()
        .find_map(|item| {
            let content = &item["content"];
            let text = content.as_str().or_else(|| content[0]["text"].as_str())?;
            let envelope: Value = serde_json::from_str(text).ok()?;
            (envelope["purpose"] == "private_activity").then(|| envelope["task_outcomes"].clone())
        })
        .unwrap();
    let acknowledgments: Value = outcomes
        .as_array()
        .unwrap()
        .iter()
        .map(|o| json!({"task_id":o["task_id"],"outcome_revision":o["outcome_revision"]}))
        .collect();
    fn replace(value: &mut Value, acks: &Value) {
        match value {
            Value::String(text) if text.contains("$observed_outcomes") => {
                let mut output: Value = serde_json::from_str(text).unwrap();
                output["activity"]["observed_outcomes"] = acks.clone();
                *text = output.to_string();
            }
            Value::Array(items) => {
                for item in items {
                    replace(item, acks);
                }
            }
            Value::Object(fields) => {
                for item in fields.values_mut() {
                    replace(item, acks);
                }
            }
            _ => {}
        }
    }
    *wire = wire
        .lines()
        .map(|line| {
            if let Some(data) = line.strip_prefix("data: ")
                && data != "[DONE]"
            {
                let mut value: Value = serde_json::from_str(data).unwrap();
                replace(&mut value, &acknowledgments);
                format!("data: {value}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n\n";
}
