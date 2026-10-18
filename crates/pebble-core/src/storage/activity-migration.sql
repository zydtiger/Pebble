-- Private activity is durable application state. It deliberately has no messages
-- table dependency: silent turns must never create a transcript bubble.
CREATE TABLE persona_activity_settings (
    persona_id TEXT PRIMARY KEY REFERENCES personas(id),
    enabled INTEGER NOT NULL DEFAULT 0,
    paused INTEGER NOT NULL DEFAULT 0,
    quiet_start_minutes INTEGER,
    quiet_end_minutes INTEGER,
    daily_wake_budget INTEGER NOT NULL DEFAULT 4,
    wakes_used INTEGER NOT NULL DEFAULT 0,
    budget_day TEXT NOT NULL DEFAULT '',
    next_wake_at TEXT,
    next_reason TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (quiet_start_minutes IS NULL OR quiet_start_minutes BETWEEN 0 AND 1439),
    CHECK (quiet_end_minutes IS NULL OR quiet_end_minutes BETWEEN 0 AND 1439),
    CHECK (daily_wake_budget BETWEEN 1 AND 24)
);
CREATE TABLE persona_activities (
    id TEXT PRIMARY KEY,
    persona_id TEXT NOT NULL REFERENCES personas(id),
    objective TEXT NOT NULL,
    progress TEXT NOT NULL DEFAULT '',
    future TEXT NOT NULL DEFAULT '[]',
    phase TEXT NOT NULL CHECK (phase IN ('idle','evaluating','exploring','assimilating','waiting','paused','stopped','failed')),
    active_task_id TEXT,
    next_wake_at TEXT,
    revision INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE UNIQUE INDEX active_persona_activity ON persona_activities(persona_id)
    WHERE phase NOT IN ('stopped','failed');
CREATE TABLE activity_events (
    id TEXT PRIMARY KEY,
    activity_id TEXT NOT NULL REFERENCES persona_activities(id),
    ordinal INTEGER NOT NULL,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    provenance TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(activity_id, ordinal)
);
CREATE TABLE activity_steps (
    id TEXT PRIMARY KEY,
    activity_id TEXT NOT NULL REFERENCES persona_activities(id),
    wake_reason TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending','running','completed','cancelled','failed')),
    task_id TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at TEXT
);
CREATE TABLE share_intents (
    id TEXT PRIMARY KEY,
    activity_id TEXT NOT NULL REFERENCES persona_activities(id),
    kind TEXT NOT NULL CHECK (kind IN ('optional','commitment')),
    destination_id TEXT,
    summary TEXT NOT NULL,
    not_before_at TEXT,
    condition TEXT NOT NULL DEFAULT 'ready' CHECK(condition IN ('ready','activity_complete')),
    requested INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('pending','fulfilled','dismissed','cancelled')),
    promise_message_id TEXT REFERENCES messages(id),
    fulfillment_message_id TEXT REFERENCES messages(id),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX activity_due ON persona_activity_settings(enabled,paused,next_wake_at);
ALTER TABLE delegated_tasks ADD COLUMN origin TEXT NOT NULL DEFAULT 'user';
ALTER TABLE delegated_tasks ADD COLUMN delivery TEXT NOT NULL DEFAULT 'room_report';
ALTER TABLE delegated_tasks ADD COLUMN activity_id TEXT;
ALTER TABLE delegated_tasks ADD COLUMN activity_step_id TEXT;
PRAGMA user_version=17;
