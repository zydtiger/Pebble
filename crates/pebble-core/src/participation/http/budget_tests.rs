use super::*;
fn config(api: &str, dialect: Option<&str>) -> ClassifierConnection {
    ClassifierConnection {
        connection: crate::ChatConnection {
            request: json!({"temperature":0,"chat_template_kwargs":{"enable_thinking":false},"vendor":{"nested":[0,false]}}).as_object().unwrap().clone(),
            model: "classifier".into(),
            base_url: "http://127.0.0.1:9/v1".into(),
            api: api.into(),
            api_key: None,
            api_key_env: None,
            responses_schema_format: dialect.map(str::to_owned),
            context_len: 100_000,
            max_output_tokens: 91,
        },
    }
}
#[test]
fn every_phase_has_complete_budget_and_configured_output_before_http() {
    for (api, dialect) in [
        ("chat_completions", None),
        ("responses", None),
        ("responses", Some("response_format")),
    ] {
        for phase in ["initial", "reassess", "intent", "permission"] {
            let mut config = config(api, dialect);
            let input = json!({"persona":{"id":"target","name":"名字"},"latest_trigger":{"message_id":"trigger","text":"问题"},"recent_real_user_transcript":[{"text":"remain silent"}],"silence_constraints":["no reply"],"latest_user_message":{"content":"大家？"}});
            let body = budgeted_body(&config, &input.to_string(), phase).unwrap();
            let estimated = serde_json::to_vec(&body).unwrap().len() as u64 + 128;
            assert_eq!(
                body[if api == "responses" {
                    "max_output_tokens"
                } else {
                    "max_tokens"
                }],
                91
            );
            assert!(body.get("context_len").is_none());
            assert_eq!(body["temperature"], 0);
            assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
            assert_eq!(body["vendor"]["nested"], json!([0, false]));
            config.connection.context_len = estimated + 91;
            budgeted_body(&config, &input.to_string(), phase).unwrap();
            config.connection.context_len -= 1;
            // classify must stop here despite the unreachable endpoint.
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let error = rt
                .block_on(classify(&config, &input.to_string(), phase))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("mandatory input") && error.contains("before HTTP"),
                "{phase}: {error}"
            );
            let mut oversized = input.clone();
            oversized["latest_trigger"]["text"] = json!("大".repeat(10000));
            assert!(budgeted_body(&config, &oversized.to_string(), phase).is_err());
        }
    }
}
#[test]
fn pruning_retains_trigger_and_same_round_predecessor_by_identity_and_permission_silence() {
    for api in ["chat_completions", "responses"] {
        for phase in ["initial", "reassess"] {
            let mut config = config(api, None);
            let mut input = json!({"persona":{"id":"target"},"latest_trigger":{"message_id":"trigger","text":"latest 你好"},"_required_message_ids":["trigger","accepted"],"recent_persona_events":[{"message_id":"old-private","text":"旧".repeat(3000)}],"recent_group_transcript":[{"message_id":"old","text":"旧".repeat(3000)},{"message_id":"trigger","text":"latest 你好"},{"message_id":"accepted","text":"newer accepted predecessor"}]});
            let mut mandatory = input.clone();
            mandatory
                .as_object_mut()
                .unwrap()
                .remove("_required_message_ids");
            mandatory["recent_persona_events"] = json!([]);
            mandatory["recent_group_transcript"]
                .as_array_mut()
                .unwrap()
                .remove(0);
            let body = request_body(&config, &mandatory, phase).unwrap();
            config.connection.context_len =
                serde_json::to_vec(&body).unwrap().len() as u64 + 128 + 91;
            let body = budgeted_body(&config, &input.to_string(), phase).unwrap();
            let text = if api == "responses" {
                &body["input"][1]["content"][0]["text"]
            } else {
                &body["messages"][1]["content"]
            };
            let final_input: Value = serde_json::from_str(text.as_str().unwrap()).unwrap();
            assert_eq!(final_input, mandatory);
            input["recent_real_user_transcript"] =
                json!([{"text":"old explicit silence".repeat(1000)}]);
            assert!(budgeted_body(&config, &input.to_string(), "permission").is_err());
        }
    }
}

#[test]
fn omission_sends_no_hidden_temperature_or_template_switch() {
    for api in ["chat_completions", "responses"] {
        let mut config = config(api, None);
        config.connection.request.clear();
        for phase in ["initial", "reassess", "intent", "permission"] {
            let body = request_body(&config, &json!({}), phase).unwrap();
            assert!(body.get("temperature").is_none());
            assert!(body.get("chat_template_kwargs").is_none());
        }
    }
}

#[test]
fn deepseek_json_object_covers_every_phase_without_relaxing_boolean_validation() {
    let mut config = config("chat_completions", None);
    config.connection.base_url = "https://api.deepseek.com/v1".into();
    config.connection.request.clear();
    for phase in ["initial", "reassess", "intent", "permission"] {
        let body = budgeted_body(&config, "{}", phase).unwrap();
        assert_eq!(body["response_format"], json!({"type":"json_object"}));
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("JSON")
        );
        let key = if phase == "intent" {
            "whole_group_question"
        } else {
            "respond"
        };
        assert!(super::super::phase_answer(&json!({key:true}).to_string(), phase).unwrap());
        for invalid in [json!({key:"true"}), json!({key:true,"extra":0}), json!({})] {
            assert!(super::super::phase_answer(&invalid.to_string(), phase).is_err());
        }
    }
}

#[test]
fn four_normalized_images_fit_16k_and_prune_optional_images_as_complete_events() {
    let root = tempfile::tempdir().unwrap();
    let pixels = image::RgbImage::from_pixel(768, 768, image::Rgb([255, 0, 0]));
    let mut bytes = Vec::new();
    pixels
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let refs = (0..4)
        .map(|_| crate::images::import(root.path(), &bytes).unwrap().1)
        .collect::<Vec<_>>();
    for api in ["chat_completions", "responses"] {
        let mut config = config(api, None);
        config.connection.context_len = 16384;
        config.connection.max_output_tokens = 256;
        for phase in ["initial", "reassess", "intent", "permission"] {
            let event = json!({"message_id":"root","speaker":{"kind":"user","id":"owner"},"text":"","image_attachments":refs});
            let mut input = json!({"_asset_root":root.path(),"_required_message_ids":["root"],"latest_trigger":{"message_id":"root","text":""}});
            let key = match phase {
                "intent" => "latest_user_message",
                "permission" => "recent_real_user_transcript",
                _ => "recent_group_transcript",
            };
            input[key] = if phase == "intent" {
                event.clone()
            } else {
                json!([event.clone()])
            };
            let body = budgeted_body(&config, &input.to_string(), phase).unwrap();
            assert_eq!(
                body.to_string().matches("data:image/png;base64,").count(),
                4,
                "{api} {phase}"
            );
            assert!(body.to_string().contains("root"));
            assert!(crate::images::estimate(&body).unwrap() > 8192);
            assert!(crate::images::estimate(&body).unwrap() < 16384 - 256);
            if phase == "initial" || phase == "reassess" {
                input["recent_persona_events"] = json!([{"message_id":"optional","text":"old".repeat(3000),"image_attachments":[refs[0]]}]);
                let trimmed = budgeted_body(&config, &input.to_string(), phase).unwrap();
                assert_eq!(
                    trimmed
                        .to_string()
                        .matches("data:image/png;base64,")
                        .count(),
                    4
                );
                assert!(!trimmed.to_string().contains("optional"));
            }
            config.connection.context_len = 4096;
            let error = budgeted_body(&config, &input.to_string(), phase)
                .unwrap_err()
                .to_string();
            assert!(error.contains("mandatory input"));
            config.connection.context_len = 16384;
        }
    }
}

#[test]
#[ignore = "explicit selected classifier; synthetic vision controls only"]
fn opt_in_selected_classifier_consumes_changed_image_pixels() {
    let root = tempfile::tempdir().unwrap();
    let mut config = config("chat_completions", None);
    config.connection.base_url =
        std::env::var("PEBBLE_CLASSIFIER_API_ROOT").expect("PEBBLE_CLASSIFIER_API_ROOT");
    config.connection.model =
        std::env::var("PEBBLE_CLASSIFIER_MODEL").expect("PEBBLE_CLASSIFIER_MODEL");
    config.connection.context_len = 16384;
    config.connection.max_output_tokens = 64;
    config.connection.request =
        json!({"temperature":0,"chat_template_kwargs":{"enable_thinking":false}})
            .as_object()
            .unwrap()
            .clone();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for (color, expected) in [([255, 0, 0], true), ([0, 0, 255], false)] {
        let pixels = image::RgbImage::from_fn(256, 256, |x, y| {
            if (x as i32 - 128).pow(2) + (y as i32 - 128).pow(2) < 80 * 80 {
                image::Rgb(color)
            } else {
                image::Rgb([255, 255, 255])
            }
        });
        let mut bytes = Vec::new();
        pixels
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let (_, reference) = crate::images::import(root.path(), &bytes).unwrap();
        let event = json!({"message_id":"root","speaker":{"kind":"user","id":"owner"},"text":"For this synthetic conversation, please respond if and only if the attached image contains a red circle. Otherwise stay silent.","image_attachments":[reference]});
        let input = json!({"_asset_root":root.path(),"_required_message_ids":["root"],"persona":{"id":"synthetic","name":"Synthetic"},"SOUL":"You are interested in red circles only. Follow the user's conditional participation request.","MEMORY_current_state":{"MEMORY":"","current_state":{}},"latest_trigger":{"message_id":"root","text":event["text"]},"recent_persona_events":[],"recent_group_transcript":[event]});
        let actual = rt
            .block_on(classify(&config, &input.to_string(), "participation"))
            .unwrap();
        assert_eq!(actual, expected, "image color {color:?}");
        println!(
            "Classifier native adapter: changed synthetic pixels {color:?} => respond={actual}"
        );
    }
}
