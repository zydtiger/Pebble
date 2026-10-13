use super::{Manager, failure};
use crate::{Error, chat::Control};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
const DEADLINE: Duration = Duration::from_secs(1800);
fn url(base: &str, route: &str) -> Result<url::Url, Error> {
    let base = url::Url::parse(&format!("{}/", base.trim_end_matches('/')))
        .map_err(|_| failure("Invalid Studio URL."))?;
    let value = base
        .join(route)
        .map_err(|_| failure("Invalid Studio route."))?;
    if value.origin() != base.origin() {
        return Err(failure("Studio artifact must use its configured origin."));
    }
    Ok(value)
}
async fn bytes(
    client: &reqwest::Client,
    method: reqwest::Method,
    base: &str,
    route: &str,
    body: Option<&Value>,
    cap: usize,
) -> Result<Vec<u8>, Error> {
    let mut request = client.request(method, url(base, route)?);
    if let Some(body) = body {
        request = request.json(body);
    }
    let mut response = request.send().await.map_err(|_| {
        failure("Studio request disconnected or timed out; submitted generation is never resent.")
    })?;
    if !response.status().is_success() {
        return Err(failure(&format!(
            "Studio returned HTTP {}.",
            response.status().as_u16()
        )));
    }
    if response.content_length().is_some_and(|v| v > cap as u64) {
        return Err(failure("Studio response exceeds its byte limit."));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("Studio response ended unexpectedly."))?
    {
        if bytes.len() + chunk.len() > cap {
            return Err(failure("Studio response exceeds its byte limit."));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn get(client: &reqwest::Client, base: &str, route: &str) -> Result<Value, Error> {
    Ok(serde_json::from_slice(
        &bytes(client, reqwest::Method::GET, base, route, None, 1024 * 1024).await?,
    )?)
}
fn identity(value: &Value, key: &str) -> Result<String, Error> {
    value[key]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 160
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .map(str::to_owned)
        .ok_or_else(|| failure("Studio returned an invalid remote identity."))
}
fn validate_profile(
    profile: &Value,
    settings: &super::StudioSettings,
    size: &super::tool::Size,
) -> Result<(), Error> {
    let multiple = profile["dimension_multiple"]
        .as_u64()
        .ok_or_else(|| failure("Studio profile has no dimension constraint."))?;
    if multiple == 0
        || u64::from(size.width) % multiple != 0
        || u64::from(size.height) % multiple != 0
    {
        return Err(failure(
            "Requested dimensions do not meet the configured Studio profile's pixel multiple. No generation was submitted.",
        ));
    }
    if let Some(steps) = settings.steps {
        let min = profile["min_steps"].as_u64().unwrap_or(1);
        let max = profile["max_steps"].as_u64().unwrap_or(100);
        if !(min..=max).contains(&u64::from(steps)) {
            return Err(failure(
                "Configured steps are incompatible with the Studio profile.",
            ));
        }
    }
    if let Some(guidance) = settings.guidance
        && (profile["guidance_fixed"]
            .as_f64()
            .is_some_and(|v| v != guidance)
            || profile["guidance_min"]
                .as_f64()
                .is_some_and(|v| guidance < v)
            || profile["guidance_max"]
                .as_f64()
                .is_some_and(|v| guidance > v))
    {
        return Err(failure(
            "Configured guidance is incompatible with the Studio profile.",
        ));
    }
    Ok(())
}
fn validate_edit_profile(profile: &Value, source: &super::source::Source) -> Result<(), Error> {
    let supports_edit = profile["generation_modes"]
        .as_array()
        .is_some_and(|m| m.iter().any(|m| m == "image-edit"));
    if !supports_edit || profile["max_input_images"].as_u64().unwrap_or(0) < 1 {
        return Err(failure(
            "Configured Studio profile does not support one-image editing. No input was uploaded.",
        ));
    }
    let bytes = profile["max_input_bytes"]
        .as_u64()
        .ok_or_else(|| failure("Studio profile has no input byte limit."))?;
    let pixels = profile["max_input_pixels"]
        .as_u64()
        .ok_or_else(|| failure("Studio profile has no input pixel limit."))?;
    if source.bytes > bytes || u64::from(source.width) * u64::from(source.height) > pixels {
        return Err(failure(
            "Original source exceeds the configured Studio profile's input limits. No input was uploaded.",
        ));
    }
    Ok(())
}
async fn upload(
    client: &reqwest::Client,
    base: &str,
    source: &super::source::Source,
    data: Vec<u8>,
) -> Result<String, Error> {
    let part = reqwest::multipart::Part::bytes(data)
        .file_name(source.filename.clone())
        .mime_str(&source.mime)
        .map_err(|_| failure("Invalid source image MIME type."))?;
    let mut response = client.post(url(base, "api/inputs")?)
        .multipart(reqwest::multipart::Form::new().part("file", part))
        .send().await.map_err(|_| failure("Studio input upload outcome is unconfirmed; it will never be repeated automatically."))?;
    if response.status() != reqwest::StatusCode::CREATED {
        return Err(failure(&format!(
            "Studio input upload returned HTTP {}.",
            response.status().as_u16()
        )));
    }
    let cap = 1024 * 1024;
    if response.content_length().is_some_and(|n| n > cap) {
        return Err(failure("Studio input response exceeds its byte limit."));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failure("Studio input upload response ended unexpectedly."))?
    {
        if body.len() + chunk.len() > cap as usize {
            return Err(failure("Studio input response exceeds its byte limit."));
        }
        body.extend_from_slice(&chunk);
    }
    identity(&serde_json::from_slice(&body)?, "input_id")
}

fn save_effective(manager: &Manager, id: &str, response: &Value) -> Result<(), Error> {
    manager.db.update_image_generation(id, |op| {
        if !op.effective_parameters.is_object() {
            op.effective_parameters = json!({});
        }
        for field in [
            "registration_id",
            "repo_id",
            "commit_sha",
            "profile",
            "dtype",
            "gpu",
            "width",
            "height",
            "steps",
            "guidance",
            "initial_seed",
            "image_count",
            "pipeline_class",
            "mode",
            "input_image_ids",
        ] {
            if let Some(value) = response.get(field) {
                op.effective_parameters[field] = value.clone();
            }
        }
        Ok(())
    })
}
pub(crate) async fn execute(
    manager: &Manager,
    id: &str,
    recover: bool,
    control: &Arc<Control>,
) -> Result<(), Error> {
    let op = manager.db.image_generation(id)?;
    let settings = op
        .settings
        .studio
        .as_ref()
        .ok_or_else(|| failure("Studio is not configured."))?;
    let client = reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| failure("Could not initialize Studio HTTP."))?;
    let until = Instant::now() + DEADLINE;
    let run = if recover {
        op.remote_id
            .clone()
            .ok_or_else(|| failure("No saved Studio run ID."))?
    } else {
        if op.dispatch_started || op.input_id.is_some() || op.remote_id.is_some() {
            return Err(failure(
                "This image operation already began dispatch. Reconcile saved identities; do not upload or submit again.",
            ));
        }
        if op.arguments.prompt.chars().count() > 8000 {
            return Err(failure("Studio prompts are limited to 8000 characters."));
        }
        let models = get(&client, &settings.base_url, "api/models").await?;
        let registration = models["registrations"]
            .as_array()
            .and_then(|models| {
                models
                    .iter()
                    .find(|m| m["id"] == settings.model_id && m["status"] == "ready")
            })
            .ok_or_else(|| failure("Configured Studio model is not ready."))?;
        let profiles = get(&client, &settings.base_url, "api/profiles").await?;
        let profile = profiles["profiles"]
            .as_array()
            .and_then(|p| {
                p.iter()
                    .find(|p| p["profile_id"] == registration["profile"])
            })
            .ok_or_else(|| failure("Studio model profile is unavailable."))?;
        validate_profile(profile, settings, &op.arguments.size)?;
        let source = if op.kind == super::ImageOperationKind::Edit {
            let source = op
                .source
                .as_ref()
                .ok_or_else(|| failure("Edit has no frozen original source."))?;
            validate_edit_profile(profile, source)?;
            Some((source, source.read(&manager.root)?))
        } else {
            None
        };
        let mut body = json!({"registration_id":settings.model_id,"gpu_uuid":settings.gpu_id,"prompt":op.arguments.prompt,"width":op.arguments.size.width,"height":op.arguments.size.height,"count":1});
        if let Some(v) = settings.steps {
            body["steps"] = json!(v);
        }
        if let Some(v) = settings.guidance {
            body["guidance"] = json!(v);
        }
        // Durable uncertainty starts before the non-idempotent HTTP submission.
        control
            .while_active(|| {
                manager.db.update_image_generation(id, |o| {
                    if o.invalidated {
                        return Err(failure("Image was cancelled before submission."));
                    }
                    o.dispatch_started = true;
                    Ok(())
                })
            })
            .ok_or_else(|| failure("Cancelled before Studio submission."))??;
        if let Some((source, data)) = source {
            manager.progress(id, "running", "Uploading the original image to Studio…")?;
            let input = upload(&client, &settings.base_url, source, data).await?;
            manager.db.update_image_generation(id, |o| {
                o.input_id = Some(input.clone());
                Ok(())
            })?;
            if control.is_cancelled() || manager.db.image_generation(id)?.invalidated {
                return Err(failure(
                    "Cancelled after source upload; no edit was submitted and the input will not be uploaded again.",
                ));
            }
            body["mode"] = json!("image-edit");
            body["input_image_ids"] = json!([input]);
        }
        let response = bytes(
            &client,
            reqwest::Method::POST,
            &settings.base_url,
            "api/generations",
            Some(&body),
            1024 * 1024,
        )
        .await?;
        let response: Value = serde_json::from_slice(&response)?;
        let run = identity(&response, "run_id")?;
        manager.db.update_image_generation(id, |o| {
            o.remote_id = Some(run.clone());
            Ok(())
        })?;
        save_effective(manager, id, &response)?;
        run
    };
    let route = format!("api/generations/{run}");
    let mut cancel_sent = false;
    loop {
        if control.is_cancelled() && !cancel_sent {
            // Acknowledgement is not proof of termination. Poll the actual run.
            let _ = tokio::time::timeout(
                Duration::from_secs(3),
                bytes(
                    &client,
                    reqwest::Method::POST,
                    &settings.base_url,
                    &format!("{route}/cancel"),
                    None,
                    1024 * 1024,
                ),
            )
            .await;
            cancel_sent = true;
        }
        if Instant::now() >= until {
            return Err(failure(
                "Studio generation exceeded 30 minutes. Reconcile its saved run; no duplicate was submitted.",
            ));
        }
        let job = if cancel_sent {
            tokio::time::timeout(
                Duration::from_secs(3),
                get(&client, &settings.base_url, &route),
            )
            .await
            .map_err(|_| {
                failure("Studio cancellation outcome is unconfirmed; reconcile the saved run.")
            })??
        } else {
            tokio::select! {biased; _=control.cancelled()=>{continue;},result=get(&client,&settings.base_url,&route)=>result?}
        };
        save_effective(manager, id, &job)?;
        match job["status"].as_str().unwrap_or("") {
            "queued" => {
                let runtime = get(&client, &settings.base_url, "api/runtime").await?;
                if runtime["current_run_id"] == run && runtime["state"] == "loading" {
                    manager.progress(id, "loading", "Loading the Studio model…")?;
                } else {
                    manager.progress(id, "queued", "Waiting in Studio's queue…")?;
                }
            }
            "running" => manager.progress(
                id,
                "running",
                if op.kind == super::ImageOperationKind::Edit {
                    "Editing an image in Studio…"
                } else {
                    "Generating an image in Studio…"
                },
            )?,
            "completed" | "partial" => {
                let completed: Vec<_> = job["images"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|a| a["status"] == "completed")
                    .collect();
                if completed.len() != 1 {
                    return Err(failure(
                        "Studio did not return exactly one completed image.",
                    ));
                }
                let artifact = identity(completed[0], "artifact_id")?;
                manager.db.update_image_generation(id, |o| {
                    o.artifact_id = Some(artifact.clone());
                    Ok(())
                })?;
                let data = bytes(
                    &client,
                    reqwest::Method::GET,
                    &settings.base_url,
                    &format!("{route}/artifacts/{artifact}"),
                    None,
                    crate::images::MAX_SOURCE_BYTES,
                )
                .await?;
                if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
                    return Err(failure("Studio artifact was not a PNG image."));
                }
                manager.import(id, &data)?;
                manager.db.update_image_generation(id, |o| {
                    if o.status != "failed" {
                        o.status = "completed".into();
                    }
                    o.activity.clear();
                    Ok(())
                })?;
                return Ok(());
            }
            "paused" | "interrupted" => {
                return terminal(
                    manager,
                    id,
                    "interrupted",
                    "Studio requires explicit recovery. Its shared queue will not be resumed automatically.",
                );
            }
            "failed" => {
                return terminal(
                    manager,
                    id,
                    "failed",
                    "Studio generation failed; no replacement or fallback was submitted.",
                );
            }
            "cancelled" => {
                return terminal(
                    manager,
                    id,
                    "cancelled",
                    "Studio confirmed cancellation. Earlier remote effects may remain.",
                );
            }
            _ => return Err(failure("Studio returned an unsupported run status.")),
        }
        if cancel_sent {
            // Avoid holding quit for an unresponsive shared service.
            return Err(failure(
                "Studio cancellation requested; remote termination is unconfirmed. Reconcile the saved run explicitly.",
            ));
        }
        tokio::select! {biased; _=control.cancelled()=>{}, _=tokio::time::sleep(Duration::from_secs(1))=>{}}
    }
}
fn terminal(manager: &Manager, id: &str, status: &str, detail: &str) -> Result<(), Error> {
    manager.db.update_image_generation(id, |o| {
        o.status = status.into();
        o.activity.clear();
        o.detail = detail.into();
        Ok(())
    })?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_validation_and_origin_constraints_precede_submission() {
        let s = super::super::StudioSettings {
            base_url: "https://api.example.com".into(),
            model_id: "model".into(),
            gpu_id: "gpu".into(),
            steps: None,
            guidance: None,
        };
        let p = json!({"dimension_multiple":32,"min_steps":1,"max_steps":100,"guidance_fixed":1.0});
        let mut size = super::super::tool::Size {
            width: 1024,
            height: 768,
        };
        assert!(validate_profile(&p, &s, &size).is_ok());
        size.width = 1000;
        assert!(validate_profile(&p, &s, &size).is_err());
        assert!(url(&s.base_url, "https://elsewhere.example/image").is_err());
        assert!(url(&s.base_url, "api/generations/run/artifacts/image-001").is_ok());
    }
}
