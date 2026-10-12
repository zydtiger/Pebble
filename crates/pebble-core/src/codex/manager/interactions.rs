use super::*;
/// Responses are derived solely from the live request and explicit native controls.
pub(super) fn response(
    pending: &TaskRequest,
    approve: Option<bool>,
    answers: &[TaskAnswer],
) -> Result<Value, Error> {
    if pending.approval {
        let accepted = approve.ok_or_else(|| failure("Choose Allow once or Deny."))?;
        return Ok(match pending.method.as_str() {
            "item/permissions/requestApproval" => {
                json!({"permissions":if accepted{pending.payload["permissions"].clone()}else{json!({})},"scope":"turn"})
            }
            "mcpServer/elicitation/request" => {
                json!({"action":if accepted{"accept"}else{"decline"}})
            }
            _ => json!({"decision":if accepted{"accept"}else{"decline"}}),
        });
    }
    if approve.is_some()
        || answers
            .iter()
            .any(|a| !pending.questions.iter().any(|q| q.id == a.question_id))
    {
        return Err(failure("Answer each pending question."));
    }
    let mut map = serde_json::Map::new();
    for question in &pending.questions {
        if question.multiple {
            let selected = answers
                .iter()
                .find(|a| a.question_id == question.id)
                .map(|a| a.answers.as_slice())
                .unwrap_or_default();
            if selected
                .iter()
                .any(|v| v.len() > 8192 || !question.choices.iter().any(|c| &c.value == v))
            {
                return Err(failure(
                    "Choose only the connected tool's displayed options.",
                ));
            }
            if question.required || !selected.is_empty() {
                map.insert(question.id.clone(), json!(selected));
            }
            continue;
        }
        let answer = answers
            .iter()
            .find(|a| a.question_id == question.id)
            .filter(|a| {
                !a.answers.is_empty()
                    && a.answers.iter().all(|s| {
                        (!question.choices.is_empty() || !s.trim().is_empty()) && s.len() <= 8192
                    })
            });
        let Some(answer) = answer else {
            if question.required {
                return Err(failure("Answer each required question within 8192 bytes."));
            }
            if answers.iter().any(|a| {
                a.question_id == question.id && a.answers.iter().any(|s| !s.trim().is_empty())
            }) {
                return Err(failure("Answer must fit within 8192 bytes."));
            }
            continue;
        };
        let value = if pending.method == "mcpServer/elicitation/request" {
            let schema = &pending.payload["requestedSchema"]["properties"][&question.id];
            match schema["type"].as_str() {
                Some("boolean" | "number" | "integer") => serde_json::from_str(&answer.answers[0])
                    .map_err(|_| {
                        failure("Enter a valid JSON value for this field's displayed type.")
                    })?,
                _ => json!(answer.answers[0]),
            }
        } else {
            json!({"answers":answer.answers})
        };
        map.insert(question.id.clone(), value);
    }
    if pending.method == "mcpServer/elicitation/request" {
        let content = Value::Object(map);
        let validator = jsonschema::options()
            .should_validate_formats(true)
            .build(&pending.payload["requestedSchema"])
            .map_err(|_| failure("This connected tool supplied an unsupported form schema."))?;
        if !validator.is_valid(&content) {
            return Err(failure(
                "The answer does not satisfy the connected tool's form. Check its displayed types, constraints and choices.",
            ));
        }
        Ok(json!({"action":"accept","content":content}))
    } else {
        Ok(json!({"answers":map}))
    }
}
