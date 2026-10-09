CREATE TABLE model_items_next (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES model_sessions(id),
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    call_id TEXT REFERENCES model_calls(id),
    kind TEXT NOT NULL CHECK (kind IN ('prefix', 'input', 'output')),
    codec TEXT NOT NULL CHECK (codec IN ('json', 'zstd')),
    payload BLOB NOT NULL,
    decoded_length INTEGER NOT NULL CHECK (decoded_length >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (session_id, sequence)
);
INSERT INTO model_items_next SELECT id, session_id, sequence, call_id, kind, codec, payload, length(payload), created_at FROM model_items;
DROP TABLE model_items;
ALTER TABLE model_items_next RENAME TO model_items;
ALTER TABLE model_sessions ADD COLUMN context_reason TEXT NOT NULL DEFAULT '';
