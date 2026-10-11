use super::live_support::isolated;
use pebble_core::{CredentialChange, Error, SettingsUpdate};
use serde_json::Value;

fn connection(scope: &str, name: &str) -> String {
    format!(
        "[{scope}{name}]\nmodel='synthetic'\nbase_url='http://localhost:9/v1'\napi='chat_completions'\ncontext_len=65536\nmax_output_tokens=64\n"
    )
}

#[test]
fn datetime_types_fail_before_json_conversion_and_native_publication_in_every_scope() {
    for persona in [false, true] {
        for name in ["chat", "classifier"] {
            let (root, core, id) = isolated();
            let target = persona.then_some(id.as_str());
            let path = if persona {
                root.path().join("personas").join(&id).join("config.toml")
            } else {
                root.path().join("config.toml")
            };
            let prefix = if persona { "" } else { "defaults." };
            let identity = if persona {
                std::fs::read_to_string(&path).unwrap()
            } else {
                String::new()
            };
            let base = format!(
                "{identity}\n{}{}",
                connection(prefix, "chat"),
                connection(prefix, "classifier")
            );
            std::fs::write(&path, &base).unwrap();
            let chat = core.settings(target).unwrap();
            let classifier = core.classifier_settings(target).unwrap();
            let chat_update = SettingsUpdate {
                chat: chat.chat,
                credential: CredentialChange::Keep,
                persona: chat.persona,
            };
            for date in [
                "2026-10-10T00:00:00Z",
                "2026-10-10T00:00:00",
                "2026-10-10",
                "12:34:56",
            ] {
                for (parameter, field) in [
                    (format!("vendor_stamp = {date}"), "vendor_stamp"),
                    (
                        format!("vendor = {{ stamps = [{{when = {date}}}] }}"),
                        "vendor.stamps[0].when",
                    ),
                    (format!("vendor = [[\"prefix\", {date}]]"), "vendor[0][1]"),
                ] {
                    let invalid = format!("{base}\n[{prefix}{name}.request]\n{parameter}\n");
                    std::fs::write(&path, &invalid).unwrap();
                    let error = core.workspace().unwrap_err();
                    let Error::Configuration {
                        path: reported,
                        detail,
                    } = error
                    else {
                        panic!("Expected configuration error");
                    };
                    assert_eq!(reported, path.display().to_string());
                    assert!(
                        detail.contains(&format!("{prefix}{name}.request.{field}")),
                        "{detail}"
                    );
                    assert!(
                        detail.contains("TOML date/time") && detail.contains("Quote"),
                        "{detail}"
                    );
                    assert!(core.settings(target).is_err());
                    assert!(core.classifier_settings(target).is_err());
                    // A native save of the other connection still validates this
                    // unsupported authored value before publishing any candidate.
                    let saved = if name == "chat" {
                        core.save_classifier_settings(target, &classifier, &CredentialChange::Keep)
                    } else {
                        core.save_settings(target, &chat_update)
                    };
                    assert!(matches!(saved, Err(Error::Configuration { .. })));
                    assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);

                    let valid = invalid.replace(date, &format!("{date:?}"));
                    std::fs::write(&path, &valid).unwrap();
                    let before = if name == "chat" {
                        let settings = core.settings(target).unwrap();
                        let before: Value =
                            serde_json::from_str(&settings.chat.request_json).unwrap();
                        core.save_settings(
                            target,
                            &SettingsUpdate {
                                chat: settings.chat,
                                credential: CredentialChange::Keep,
                                persona: settings.persona,
                            },
                        )
                        .unwrap();
                        before
                    } else {
                        let settings = core.classifier_settings(target).unwrap();
                        let before: Value =
                            serde_json::from_str(&settings.connection.request_json).unwrap();
                        core.save_classifier_settings(target, &settings, &CredentialChange::Keep)
                            .unwrap();
                        before
                    };
                    let read = if name == "chat" {
                        core.settings(target).unwrap().chat.request_json
                    } else {
                        core.classifier_settings(target)
                            .unwrap()
                            .connection
                            .request_json
                    };
                    let after: Value = serde_json::from_str(&read).unwrap();
                    assert_eq!(after, before);
                    let leaf = match field {
                        "vendor_stamp" => &after["vendor_stamp"],
                        "vendor.stamps[0].when" => &after["vendor"]["stamps"][0]["when"],
                        _ => &after["vendor"][0][1],
                    };
                    assert_eq!(leaf.as_str(), Some(date));
                    assert!(
                        !std::fs::read_to_string(&path)
                            .unwrap()
                            .contains("$__toml_private_datetime")
                    );
                }
            }
        }
    }
}
