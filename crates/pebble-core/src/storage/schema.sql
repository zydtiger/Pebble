CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    sender_kind TEXT NOT NULL CHECK (sender_kind IN ('user', 'persona')),
    sender_id TEXT NOT NULL,
    content TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('complete', 'streaming', 'cancelled', 'interrupted', 'failed')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (conversation_id, sequence)
);
CREATE TABLE persona_events (
    id TEXT PRIMARY KEY,
    persona_id TEXT NOT NULL REFERENCES personas(id),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    message_id TEXT NOT NULL REFERENCES messages(id),
    kind TEXT NOT NULL CHECK (kind IN ('received', 'sent')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (persona_id, sequence),
    UNIQUE (persona_id, message_id)
);
CREATE TABLE model_sessions (
    id TEXT PRIMARY KEY,
    persona_id TEXT NOT NULL REFERENCES personas(id),
    protocol TEXT NOT NULL,
    model TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'retired')),
    covered_event_sequence INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE UNIQUE INDEX active_session ON model_sessions(persona_id) WHERE status = 'active';
CREATE TABLE model_calls (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES model_sessions(id),
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    trigger_message_id TEXT NOT NULL REFERENCES messages(id),
    reply_message_id TEXT NOT NULL UNIQUE REFERENCES messages(id),
    input_last_sequence INTEGER NOT NULL,
    event_cutoff INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('streaming', 'completed', 'cancelled', 'interrupted', 'failed')),
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    finished_at TEXT
);
CREATE UNIQUE INDEX active_call ON model_calls(session_id) WHERE status = 'streaming';
CREATE TABLE model_items (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES model_sessions(id),
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    call_id TEXT REFERENCES model_calls(id),
    kind TEXT NOT NULL CHECK (kind IN ('prefix', 'input', 'output')),
    codec TEXT NOT NULL CHECK (codec = 'json'),
    payload BLOB NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (session_id, sequence)
);
CREATE TABLE model_item_events (
    item_id TEXT NOT NULL REFERENCES model_items(id),
    event_id TEXT NOT NULL REFERENCES persona_events(id),
    PRIMARY KEY (item_id, event_id)
);
CREATE INDEX event_items ON model_item_events(event_id);
