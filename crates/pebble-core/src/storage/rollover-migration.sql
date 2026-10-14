ALTER TABLE model_sessions ADD COLUMN history_tools INTEGER NOT NULL DEFAULT 0;
ALTER TABLE model_calls ADD COLUMN history_cutoff INTEGER;
ALTER TABLE context_maintenance ADD COLUMN automatic INTEGER NOT NULL DEFAULT 0;
CREATE TABLE summarized_events (
 session_id TEXT NOT NULL REFERENCES model_sessions(id),
 event_id TEXT NOT NULL REFERENCES persona_events(id),
 PRIMARY KEY(session_id,event_id)
);
CREATE TABLE automatic_rollovers (
 source_session TEXT NOT NULL REFERENCES model_sessions(id),
 last_item_sequence INTEGER NOT NULL,
 PRIMARY KEY(source_session,last_item_sequence)
);
CREATE TABLE session_recent_messages (
 session_id TEXT NOT NULL REFERENCES model_sessions(id),
 position INTEGER NOT NULL CHECK(position >= 0 AND position < 10),
 message_id TEXT NOT NULL REFERENCES messages(id),
 speaker_name TEXT NOT NULL,
 room_kind TEXT NOT NULL,
 room_name TEXT NOT NULL,
 PRIMARY KEY(session_id,position),
 UNIQUE(session_id,message_id)
);
PRAGMA user_version=16;
