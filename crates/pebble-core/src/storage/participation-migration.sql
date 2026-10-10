CREATE TABLE participation_decisions (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES messages(id),
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    persona_id TEXT NOT NULL REFERENCES personas(id),
    membership_id TEXT NOT NULL REFERENCES conversation_memberships(id),
    event_cutoff INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    respond INTEGER CHECK(respond IN (0,1)),
    status TEXT NOT NULL CHECK(status IN ('pending','mentioned','unconfigured','listening','selected','failed','cancelled','interrupted')),
    error TEXT NOT NULL DEFAULT '',
    call_id TEXT REFERENCES model_calls(id),
    elapsed_ms INTEGER,
    UNIQUE(message_id,persona_id)
);
CREATE INDEX participation_room ON participation_decisions(conversation_id,message_id);
PRAGMA user_version=9;
