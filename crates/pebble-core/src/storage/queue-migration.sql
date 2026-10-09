CREATE TABLE model_calls_next (
    id TEXT PRIMARY KEY,
    session_id TEXT REFERENCES model_sessions(id),
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    trigger_message_id TEXT NOT NULL REFERENCES messages(id),
    reply_message_id TEXT NOT NULL UNIQUE REFERENCES messages(id),
    input_last_sequence INTEGER,
    event_cutoff INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued', 'streaming', 'completed', 'cancelled', 'interrupted', 'failed')),
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at TEXT
);
INSERT INTO model_calls_next SELECT * FROM model_calls;
DROP TABLE model_calls;
ALTER TABLE model_calls_next RENAME TO model_calls;
CREATE UNIQUE INDEX active_call ON model_calls(session_id) WHERE status = 'streaming';
