CREATE TABLE conversations_next (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('direct','group')),
    persona_id TEXT UNIQUE REFERENCES personas(id),
    title TEXT NOT NULL DEFAULT '',
    publication_sequence INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((kind='direct' AND persona_id IS NOT NULL) OR (kind='group' AND persona_id IS NULL))
);
INSERT INTO conversations_next(id,kind,persona_id,created_at) SELECT id,kind,persona_id,created_at FROM conversations;
DROP TABLE conversations;
ALTER TABLE conversations_next RENAME TO conversations;
ALTER TABLE messages ADD COLUMN publication_sequence INTEGER;
UPDATE messages SET publication_sequence=sequence WHERE status='complete';
UPDATE conversations SET publication_sequence=COALESCE((SELECT MAX(publication_sequence) FROM messages WHERE conversation_id=conversations.id),0);
CREATE UNIQUE INDEX message_publication ON messages(conversation_id,publication_sequence);
CREATE TABLE conversation_memberships (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    persona_id TEXT NOT NULL REFERENCES personas(id),
    ordinal INTEGER NOT NULL,
    joined_publication INTEGER NOT NULL,
    left_publication INTEGER,
    CHECK (joined_publication>=0 AND (left_publication IS NULL OR left_publication>=joined_publication)),
    UNIQUE(conversation_id,ordinal)
);
CREATE UNIQUE INDEX current_member ON conversation_memberships(conversation_id,persona_id) WHERE left_publication IS NULL;
CREATE INDEX persona_memberships ON conversation_memberships(persona_id,conversation_id);
CREATE TABLE message_mentions (
    message_id TEXT NOT NULL REFERENCES messages(id),
    persona_id TEXT NOT NULL REFERENCES personas(id),
    membership_id TEXT NOT NULL REFERENCES conversation_memberships(id),
    PRIMARY KEY(message_id,persona_id)
);
ALTER TABLE model_calls ADD COLUMN persona_id TEXT REFERENCES personas(id);
ALTER TABLE model_calls ADD COLUMN persona_sequence INTEGER;
ALTER TABLE model_calls ADD COLUMN membership_id TEXT REFERENCES conversation_memberships(id);
UPDATE model_calls SET persona_id=(SELECT persona_id FROM conversations WHERE id=model_calls.conversation_id);
UPDATE model_calls SET persona_sequence=(SELECT sequence FROM messages WHERE id=model_calls.reply_message_id);
CREATE UNIQUE INDEX persona_call_order ON model_calls(persona_id,persona_sequence);
CREATE INDEX pending_persona_calls ON model_calls(persona_id,status);
PRAGMA user_version=8;
