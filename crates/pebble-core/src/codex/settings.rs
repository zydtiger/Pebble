use crate::{Error, file_error};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// Previously saved experimental tasks keep their restrictive scope marker.
// New tasks use the ordinary settings; old scopes are never silently widened.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionScope {
    PublicWebResearch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CodexSettings {
    pub enabled: bool,
    pub executable: String,
    pub writable: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
}
impl Default for CodexSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            executable: "codex".into(),
            writable: true,
            model: None,
            effort: None,
        }
    }
}
impl CodexSettings {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.executable.trim().is_empty() {
            return Err(super::failure("Choose the local Codex executable."));
        }
        if self.model.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(super::failure(
                "Leave model empty to inherit local Codex configuration.",
            ));
        }
        if self.effort.as_deref().is_some_and(|s| {
            ![
                "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
            ]
            .contains(&s)
        }) {
            return Err(super::failure(
                "Choose a supported reasoning effort or inherit Codex settings.",
            ));
        }
        Ok(())
    }
}

// Keep the existing flat task snapshot, independently of editable app settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskSettings {
    pub enabled: bool,
    pub executable: String,
    pub workspace: String,
    pub writable: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ExecutionScope>,
}
impl TaskSettings {
    pub(crate) fn new(settings: &CodexSettings, workspace: &Path) -> Self {
        Self {
            enabled: settings.enabled,
            executable: settings.executable.clone(),
            workspace: workspace.display().to_string(),
            writable: settings.writable,
            model: settings.model.clone(),
            effort: settings.effort.clone(),
            scope: None,
        }
    }
    pub(crate) fn cwd(&self) -> Result<PathBuf, Error> {
        let path = Path::new(&self.workspace);
        if !path.is_absolute() || !path.is_dir() {
            return Err(super::failure("The saved task workspace is unavailable."));
        }
        path.canonicalize().map_err(|e| file_error(path, e))
    }
}

pub(crate) fn read(root: &Path) -> Result<CodexSettings, Error> {
    #[derive(Deserialize)]
    struct Config {
        #[serde(default)]
        codex: CodexSettings,
    }
    let path = root.join("config.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| file_error(&path, e))?;
    let config: Config = toml::from_str(&text).map_err(|e| Error::Configuration {
        path: path.display().to_string(),
        detail: e.to_string(),
    })?;
    config.codex.validate()?;
    Ok(config.codex)
}
pub(crate) fn save(root: &Path, settings: &CodexSettings) -> Result<(), Error> {
    settings.validate()?;
    let path = root.join("config.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| file_error(&path, e))?;
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| super::failure(&e.to_string()))?;
    let value = toml::to_string(settings).map_err(|e| super::failure(&e.to_string()))?;
    let table = value
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| super::failure(&e.to_string()))?;
    doc["codex"] = toml_edit::Item::Table(table.as_table().clone());
    crate::settings::files::publish(&[(path, doc.to_string())])
}

/// Finder does not necessarily inherit a developer shell's PATH.
pub(crate) fn executable(configured: &str) -> Result<PathBuf, Error> {
    if configured.contains('/') {
        let path = PathBuf::from(configured);
        if path.is_absolute() && path.is_file() {
            return Ok(path);
        }
        return Err(super::failure(
            "The selected Codex executable is unavailable.",
        ));
    }
    let mut candidates = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(configured))
        .collect::<Vec<_>>();
    if configured == "codex" {
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            for relative in [".nix-profile/bin/codex", ".local/bin/codex"] {
                candidates.push(home.join(relative));
            }
        }
        for path in ["/opt/homebrew/bin/codex", "/usr/local/bin/codex"] {
            candidates.push(PathBuf::from(path));
        }
    }
    candidates.into_iter().find(|p|p.is_file()).ok_or_else(||super::failure("Codex was not found. Install/login with the local Codex CLI, or select its executable in Codex settings."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_workspace_defaults_to_writes_and_explicit_read_only_survives_reload() {
        let settings: CodexSettings = toml::from_str("").unwrap();
        assert!(settings.writable);
        settings.validate().unwrap();
        let readonly: CodexSettings = toml::from_str("writable = false").unwrap();
        readonly.validate().unwrap();
        let frozen = serde_json::to_string(&readonly).unwrap();
        assert!(
            !serde_json::from_str::<CodexSettings>(&frozen)
                .unwrap()
                .writable
        );
        assert!(toml::from_str::<CodexSettings>("workspace = '/external'").is_err());
        let legacy = serde_json::json!({"enabled":true,"executable":"codex","workspace":"/old/task/path","writable":false,"model":null,"effort":null});
        let saved: TaskSettings = serde_json::from_value(legacy.clone()).unwrap();
        assert!(!saved.writable);
        assert_eq!(saved.workspace, "/old/task/path");
        assert_eq!(serde_json::to_value(saved).unwrap(), legacy);
    }
}
