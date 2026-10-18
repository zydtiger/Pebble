//! Persona TOML is the sole authority for private activity policy.
use crate::{Error, settings};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use toml_edit::value;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(from = "StoredHeartbeat")]
pub(crate) struct Heartbeat {
    pub enabled: bool,
    pub daily_wake_budget: u8,
    pub quiet_start: Option<String>,
    pub quiet_end: Option<String>,
}
// Read the former field only to preserve disabled intent during upgrade.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct StoredHeartbeat {
    enabled: bool,
    paused: bool,
    daily_wake_budget: u8,
    quiet_start: Option<String>,
    quiet_end: Option<String>,
}
impl Default for StoredHeartbeat {
    fn default() -> Self {
        Self {
            enabled: false,
            paused: false,
            daily_wake_budget: 4,
            quiet_start: None,
            quiet_end: None,
        }
    }
}
impl From<StoredHeartbeat> for Heartbeat {
    fn from(stored: StoredHeartbeat) -> Self {
        Self {
            enabled: stored.enabled && !stored.paused,
            daily_wake_budget: stored.daily_wake_budget,
            quiet_start: stored.quiet_start,
            quiet_end: stored.quiet_end,
        }
    }
}
impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            enabled: false,
            daily_wake_budget: 4,
            quiet_start: None,
            quiet_end: None,
        }
    }
}
impl Heartbeat {
    pub(crate) fn active(&self) -> bool {
        self.enabled
    }
    pub(crate) fn quiet_minutes(&self) -> (Option<u16>, Option<u16>) {
        (
            self.quiet_start.as_deref().and_then(minutes),
            self.quiet_end.as_deref().and_then(minutes),
        )
    }
    pub(crate) fn validate(&self, path: &Path) -> Result<(), Error> {
        let fail = |detail: &str| Error::Configuration {
            path: path.display().to_string(),
            detail: detail.into(),
        };
        if !(1..=24).contains(&self.daily_wake_budget) {
            return Err(fail(
                "heartbeat.daily_wake_budget must be between 1 and 24.",
            ));
        }
        match (&self.quiet_start, &self.quiet_end) {
            (None, None) => Ok(()),
            (Some(start), Some(end))
                if minutes(start).is_some() && minutes(end).is_some() && start != end =>
            {
                Ok(())
            }
            _ => Err(fail(
                "Set both heartbeat.quiet_start and quiet_end to distinct local HH:MM times (00:00–23:59), or omit both.",
            )),
        }
    }
    pub(crate) fn from_controls(
        enabled: bool,
        start: Option<u16>,
        end: Option<u16>,
        budget: u8,
    ) -> Self {
        let time = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);
        Self {
            enabled,
            daily_wake_budget: budget,
            quiet_start: start.map(time),
            quiet_end: end.map(time),
        }
    }
}
fn minutes(time: &str) -> Option<u16> {
    let bytes = time.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || ![bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
    {
        return None;
    }
    let hour = time[..2].parse::<u16>().ok()?;
    let minute = time[3..].parse::<u16>().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}
pub(crate) fn path(root: &Path, persona: &str) -> Result<PathBuf, Error> {
    if uuid::Uuid::parse_str(persona).is_err() {
        return Err(Error::UnknownPersona);
    }
    let path = root.join("personas").join(persona).join("config.toml");
    if !path.is_file() {
        return Err(Error::UnknownPersona);
    }
    Ok(path)
}
pub(crate) fn read(root: &Path, persona: &str) -> Result<Heartbeat, Error> {
    let path = path(root, persona)?;
    let doc = settings::document(&path)?;
    read_document(&doc, &path)
}
fn read_document(doc: &toml_edit::DocumentMut, path: &Path) -> Result<Heartbeat, Error> {
    #[derive(Deserialize)]
    struct Config {
        #[serde(default)]
        heartbeat: Heartbeat,
    }
    let config: Config = toml::from_str(&doc.to_string()).map_err(|_| Error::Configuration {
        path: path.display().to_string(),
        detail: "Invalid heartbeat configuration; check fields and value types.".into(),
    })?;
    config.heartbeat.validate(path)?;
    Ok(config.heartbeat)
}
/// Called at startup under the exclusive data lock, before any worker starts.
pub(crate) fn remove_legacy_pause(root: &Path, persona: &str) -> Result<(), Error> {
    let path = path(root, persona)?;
    let mut doc = settings::document(&path)?;
    let policy = read_document(&doc, &path)?;
    if let Some(table) = doc
        .get_mut("heartbeat")
        .and_then(toml_edit::Item::as_table_like_mut)
        && table.remove("paused").is_some()
    {
        table.insert("enabled", value(policy.enabled));
        settings::files::publish(&[(path, doc.to_string())])?;
    }
    Ok(())
}
pub(crate) fn write(root: &Path, persona: &str, policy: &Heartbeat) -> Result<(), Error> {
    let path = path(root, persona)?;
    policy.validate(&path)?;
    let mut doc = settings::document(&path)?;
    // Preserve unrelated tables, credentials and comments.
    if doc.get("heartbeat").is_none() {
        doc["heartbeat"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let table = doc["heartbeat"]
        .as_table_like_mut()
        .ok_or_else(|| Error::Configuration {
            path: path.display().to_string(),
            detail: "heartbeat must be a TOML table.".into(),
        })?;
    table.insert("enabled", value(policy.enabled));
    table.remove("paused");
    table.insert(
        "daily_wake_budget",
        value(i64::from(policy.daily_wake_budget)),
    );
    for (key, time) in [
        ("quiet_start", &policy.quiet_start),
        ("quiet_end", &policy.quiet_end),
    ] {
        if let Some(time) = time {
            table.insert(key, value(time));
        } else {
            table.remove(key);
        }
    }
    let candidates = [(path, doc.to_string())];
    crate::config::load_candidate(root, &candidates)?;
    settings::files::publish(&candidates)
}

/// Files publish before the schema commits. Retrying a partial migration keeps
/// any already published table, so explicit file settings always win.
pub(crate) fn migrate(db: &rusqlite::Connection, root: &Path) -> Result<(), Error> {
    let mut query = db.prepare("SELECT persona_id,enabled,paused,quiet_start_minutes,quiet_end_minutes,daily_wake_budget FROM persona_activity_settings")?;
    let rows = query.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Heartbeat::from_controls(
                r.get::<_, bool>(1)? && !r.get::<_, bool>(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ),
        ))
    })?;
    for row in rows {
        let (persona, policy) = row?;
        let path = path(root, &persona)?;
        let doc = settings::document(&path)?;
        if doc.contains_key("heartbeat") {
            read_document(&doc, &path)?;
        } else {
            write(root, &persona, &policy)?;
        }
    }
    db.execute_batch("CREATE TABLE persona_activity_runtime (
        persona_id TEXT PRIMARY KEY REFERENCES personas(id),
        wakes_used INTEGER NOT NULL DEFAULT 0,
        budget_day TEXT NOT NULL DEFAULT '',
        next_wake_at TEXT,
        next_reason TEXT NOT NULL DEFAULT '',
        updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
    );
    INSERT INTO persona_activity_runtime SELECT persona_id,wakes_used,budget_day,next_wake_at,next_reason,updated_at FROM persona_activity_settings;
    DROP TABLE persona_activity_settings;
    CREATE INDEX activity_due ON persona_activity_runtime(next_wake_at);
    PRAGMA user_version=21;")?;
    Ok(())
}

pub(crate) const DEFAULT_CONFIG: &str = "\n[heartbeat]\nenabled = false\ndaily_wake_budget = 4\n# Local quiet hours; omit both keys to disable.\nquiet_start = \"23:00\"\nquiet_end = \"08:00\"\n";
