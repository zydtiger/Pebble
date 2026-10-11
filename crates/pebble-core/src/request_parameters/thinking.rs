use serde_json::{Map, Value};

/// Enablement, effort and budget are separate dimensions. Reject contradictory
/// settings rather than pretending every backend exposes one universal control.
pub(super) fn validate(params: &Map<String, Value>) -> Result<(), String> {
    let mut states = Vec::new();
    let mut efforts = Vec::new();
    controls(params, &mut states, &mut efforts);
    if let Some(template) = params
        .get("chat_template_kwargs")
        .and_then(Value::as_object)
    {
        controls(template, &mut states, &mut efforts);
    }
    if states.contains(&true) && states.contains(&false) {
        return Err("Thinking controls conflict: disabled/none/zero cannot be combined with enabled thinking, active effort, or a positive/unlimited thinking budget.".into());
    }
    if efforts.windows(2).any(|v| v[0] != v[1]) {
        return Err(
            "Thinking effort selectors disagree; configure one effective effort value.".into(),
        );
    }
    Ok(())
}
fn controls<'a>(
    params: &'a Map<String, Value>,
    states: &mut Vec<bool>,
    efforts: &mut Vec<&'a str>,
) {
    if let Some(enabled) = params.get("enable_thinking").and_then(Value::as_bool) {
        states.push(enabled);
    }
    for value in [
        params.get("reasoning_effort"),
        params.get("reasoning").and_then(|v| v.get("effort")),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(effort) = value.as_str() {
            states.push(effort != "none");
            efforts.push(effort);
        }
    }
    if let Some(thinking) = params.get("thinking") {
        if let Some(kind) = thinking.get("type").and_then(Value::as_str) {
            states.push(kind != "disabled");
        }
        if let Some(budget) = thinking.get("budget_tokens").and_then(Value::as_u64) {
            states.push(budget > 0);
        }
    }
    for key in ["thinking_budget", "reasoning_budget"] {
        if let Some(budget) = params.get(key).and_then(Value::as_i64) {
            states.push(budget != 0);
        }
    }
}

pub(super) fn reasoning_valid(value: &Value) -> bool {
    value.as_object().is_some_and(|v| {
        v.iter().all(|(k, v)| match k.as_str() {
            "effort" => super::effort(v),
            "summary" => matches!(v.as_str(), Some("auto" | "concise" | "detailed")),
            _ => false,
        })
    })
}
pub(super) fn thinking_valid(value: &Value) -> bool {
    value.as_object().is_some_and(|v| {
        v.contains_key("type")
            && v.iter().all(|(k, v)| match k.as_str() {
                "type" => matches!(v.as_str(), Some("enabled" | "disabled" | "adaptive")),
                "budget_tokens" => v.as_u64().is_some_and(|v| v > 0),
                _ => false,
            })
    })
}
