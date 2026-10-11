ALTER TABLE model_calls ADD COLUMN memory_snapshot TEXT;
ALTER TABLE model_calls ADD COLUMN native_tool_output TEXT;
ALTER TABLE model_sessions ADD COLUMN memory_tools INTEGER NOT NULL DEFAULT 0;
CREATE TABLE memory_operations (
 id TEXT PRIMARY KEY, owner_id TEXT NOT NULL, persona_id TEXT NOT NULL REFERENCES personas(id),
 session_id TEXT REFERENCES model_sessions(id), protocol TEXT NOT NULL, provider_call_id TEXT NOT NULL,
 arguments TEXT NOT NULL, previous_content TEXT NOT NULL, content TEXT NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('prepared','committed','failed')),
 result TEXT, result_item_id TEXT REFERENCES model_items(id),
 UNIQUE(owner_id,provider_call_id)
);
CREATE TABLE context_maintenance (
 id TEXT PRIMARY KEY, persona_id TEXT NOT NULL REFERENCES personas(id),
 source_session TEXT REFERENCES model_sessions(id), event_cutoff INTEGER NOT NULL,
 state_before TEXT NOT NULL, binding TEXT NOT NULL, status TEXT NOT NULL,
 detail TEXT NOT NULL DEFAULT '', request TEXT, native_output TEXT, native_result TEXT,
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE UNIQUE INDEX active_maintenance ON context_maintenance(persona_id) WHERE status='running';
CREATE TABLE context_markers (
 id TEXT PRIMARY KEY, persona_id TEXT NOT NULL REFERENCES personas(id),
 session_id TEXT NOT NULL UNIQUE REFERENCES model_sessions(id),
 created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE context_marker_rooms (
 marker_id TEXT NOT NULL REFERENCES context_markers(id),
 conversation_id TEXT NOT NULL REFERENCES conversations(id), after_sequence INTEGER NOT NULL,
 PRIMARY KEY(marker_id,conversation_id)
);
PRAGMA user_version=11;
