CREATE TABLE activity_calls (
    id TEXT PRIMARY KEY,
    step_id TEXT NOT NULL UNIQUE REFERENCES activity_steps(id),
    activity_id TEXT NOT NULL REFERENCES persona_activities(id),
    persona_id TEXT NOT NULL REFERENCES personas(id),
    session_id TEXT NOT NULL REFERENCES model_sessions(id),
    status TEXT NOT NULL CHECK (status IN ('streaming','completed','cancelled','failed','interrupted')),
    state_before TEXT NOT NULL,
    purpose TEXT NOT NULL DEFAULT 'private_activity',
    share_intent_id TEXT REFERENCES share_intents(id),
    published_message_id TEXT REFERENCES messages(id),
    input_last_sequence INTEGER NOT NULL,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    finished_at TEXT
);
CREATE UNIQUE INDEX active_persona_activity_call ON activity_calls(persona_id) WHERE status='streaming';
ALTER TABLE model_items ADD COLUMN activity_call_id TEXT REFERENCES activity_calls(id);
CREATE INDEX model_items_activity_call ON model_items(activity_call_id);
PRAGMA user_version=18;
