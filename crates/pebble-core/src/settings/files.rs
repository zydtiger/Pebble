use crate::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
struct Staged {
    path: PathBuf,
    target: PathBuf,
}
impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
pub(crate) fn publish(candidates: &[(PathBuf, String)]) -> Result<(), Error> {
    let mut staged = Vec::new();
    for (target, text) in candidates {
        if std::fs::metadata(target).is_ok_and(|m| !m.is_file()) {
            return Err(failed(&[], target));
        }
        let path = target
            .parent()
            .unwrap()
            .join(format!(".pebble-save-{}", uuid::Uuid::new_v4()));
        let entry = Staged {
            path: path.clone(),
            target: target.clone(),
        };
        let result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            if let Ok(metadata) = std::fs::metadata(target) {
                file.set_permissions(metadata.permissions())?;
            }
            file.write_all(text.as_bytes())?;
            file.sync_all()
        })();
        if result.is_err() {
            return Err(failed(&[], target));
        }
        staged.push(entry);
    }
    let mut saved = Vec::new();
    for entry in &staged {
        if std::fs::rename(&entry.path, &entry.target).is_err() {
            return Err(failed(&saved, &entry.target));
        }
        saved.push(
            entry
                .target
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
    }
    Ok(())
}
fn failed(saved: &[String], target: &Path) -> Error {
    Error::SettingsSave(if saved.is_empty() {
        format!(
            "Could not save {}. No files changed; check the character/configuration files.",
            target.file_name().unwrap().to_string_lossy()
        )
    } else {
        format!(
            "Saved {}, but could not save {}. This save was partial; reopen settings or inspect the files before trying again.",
            saved.join(", "),
            target.file_name().unwrap().to_string_lossy()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn later_staging_failure_keeps_all_authoritative_files_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("SOUL.md");
        let second = root.path().join("MEMORY.md");
        std::fs::write(&first, "original").unwrap();
        std::fs::create_dir(&second).unwrap();
        let error = publish(&[
            (first.clone(), "replacement".into()),
            (second, "memory".into()),
        ])
        .unwrap_err();
        assert!(error.to_string().contains("No files changed"));
        assert_eq!(std::fs::read_to_string(first).unwrap(), "original");
        assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".pebble-save-")
        }));
    }
}
