use super::*;

/// Only the primitive and choice shapes of the MCP elicitation protocol are native forms.
pub(super) fn form_questions(schema: &Value) -> Option<Vec<TaskQuestion>> {
    if schema["type"] != "object"
        || schema.as_object()?.keys().any(|k| {
            ![
                "type",
                "properties",
                "required",
                "$schema",
                "additionalProperties",
            ]
            .contains(&k.as_str())
        })
    {
        return None;
    }
    jsonschema::options()
        .should_validate_formats(true)
        .build(schema)
        .ok()?;
    schema["properties"]
        .as_object()?
        .iter()
        .map(|(id, field)| {
            let guidance = field_guidance(field)?;
            let multiple = field["type"] == "array";
            let choices = if multiple {
                if field["items"]
                    .as_object()?
                    .keys()
                    .any(|k| !["type", "enum", "anyOf"].contains(&k.as_str()))
                {
                    return None;
                }
                choices(&field["items"], "anyOf")?
            } else if field["enum"].is_array() || field["oneOf"].is_array() {
                choices(field, "oneOf")?
            } else {
                if !matches!(
                    field["type"].as_str(),
                    Some("string" | "boolean" | "integer" | "number")
                ) || field.get("anyOf").is_some()
                    || field.get("oneOf").is_some()
                {
                    return None;
                }
                Vec::new()
            };
            Some(TaskQuestion {
                id: id.clone(),
                required: schema["required"]
                    .as_array()
                    .is_some_and(|r| r.iter().any(|v| v.as_str() == Some(id))),
                text: format!(
                    "{} ({})",
                    field["title"].as_str().unwrap_or(id),
                    field["type"].as_str().unwrap_or("string")
                ),
                header: String::new(),
                description: field["description"].as_str().unwrap_or_default().into(),
                guidance,
                secret: field["writeOnly"] == true,
                options: Vec::new(),
                option_descriptions: Vec::new(),
                choices,
                multiple,
            })
        })
        .collect()
}

/// Captions cover the supplied protocol's primitive constraints, not general JSON Schema.
fn field_guidance(field: &Value) -> Option<Vec<String>> {
    let allowed: &[&str] = match field["type"].as_str()? {
        "string" => &[
            "enum",
            "enumNames",
            "oneOf",
            "minLength",
            "maxLength",
            "format",
        ],
        "number" | "integer" => &["minimum", "maximum"],
        "array" => &["items", "minItems", "maxItems"],
        "boolean" => &[],
        _ => return None,
    };
    if field.as_object()?.keys().any(|key| {
        !["type", "title", "description", "default", "writeOnly"].contains(&key.as_str())
            && !allowed.contains(&key.as_str())
    }) {
        return None;
    }
    let mut captions = Vec::new();
    for (key, label) in [
        ("minimum", "Minimum"),
        ("maximum", "Maximum"),
        ("minLength", "Minimum length (characters)"),
        ("maxLength", "Maximum length (characters)"),
        ("minItems", "Minimum selections"),
        ("maxItems", "Maximum selections"),
    ] {
        if let Some(value) = field.get(key).filter(|v| !v.is_null()) {
            captions.push(format!("{label}: {value}"));
        }
    }
    if let Some(format) = field["format"].as_str() {
        if !["email", "uri", "date", "date-time"].contains(&format) {
            return None;
        }
        captions.push(format!("Format: {format}"));
    }
    if let Some(default) = field.get("default").filter(|v| !v.is_null()) {
        captions.push(if field["writeOnly"] == true {
            "A tool default exists; enter an explicit secret answer.".into()
        } else {
            format!("Suggested default: {default}")
        });
    }
    Some(captions)
}

fn choices(field: &Value, alternatives: &str) -> Option<Vec<TaskChoice>> {
    let values = if let Some(values) = field["enum"].as_array() {
        values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                let value = value.as_str()?;
                Some(TaskChoice {
                    value: value.into(),
                    label: field["enumNames"][i].as_str().unwrap_or(value).into(),
                })
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        field[alternatives]
            .as_array()?
            .iter()
            .map(|option| {
                if option
                    .as_object()?
                    .keys()
                    .any(|k| !["const", "title"].contains(&k.as_str()))
                {
                    return None;
                }
                let value = option["const"].as_str()?;
                Some(TaskChoice {
                    value: value.into(),
                    label: option["title"].as_str().unwrap_or(value).into(),
                })
            })
            .collect::<Option<Vec<_>>>()?
    };
    (!values.is_empty()).then_some(values)
}

pub(super) fn command_summary(params: &Value) -> String {
    let mut lines = vec![if params["kind"] == "writeStdin" {
        "Send input to an existing terminal".to_owned()
    } else {
        "Execute a command".to_owned()
    }];
    if let Some(command) = params["command"].as_str() {
        lines.push(format!("Command: {command}"));
    }
    for action in params["commandActions"].as_array().into_iter().flatten() {
        let kind = match action["type"].as_str() {
            Some("read") => "Read file",
            Some("listFiles") => "List files",
            Some("search") => "Search",
            _ => "Parsed command",
        };
        let mut parts = vec![kind.to_owned()];
        for (key, label) in [
            ("name", "Name"),
            ("path", "Path"),
            ("query", "Pattern"),
            ("command", "Command"),
        ] {
            if let Some(value) = action[key].as_str() {
                parts.push(format!("{label}: {value}"));
            }
        }
        lines.push(parts.join(" · "));
    }
    if lines.len() == 1 {
        lines.push("Codex did not supply operation text or parsed action details.".into());
    }
    if let Some(network) = params["networkApprovalContext"].as_object() {
        for (key, label) in [("host", "Network host"), ("protocol", "Network protocol")] {
            if let Some(value) = network.get(key).and_then(Value::as_str) {
                lines.push(format!("{label}: {value}"));
            }
        }
    }
    lines.join("\n")
}
