//! Restore the actual v0.1.2 schema before exercising older-version upgrades.
pub fn v7(root: &std::path::Path) {
    let db = rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF;
        DROP TABLE participation_decisions;
        DROP INDEX persona_call_order; DROP INDEX pending_persona_calls;
        ALTER TABLE model_calls DROP COLUMN membership_id;
        ALTER TABLE model_calls DROP COLUMN persona_sequence;
        ALTER TABLE model_calls DROP COLUMN persona_id;
        DROP TABLE message_mentions; DROP TABLE conversation_memberships;
        DROP INDEX message_publication; ALTER TABLE messages DROP COLUMN publication_sequence;
        CREATE TABLE old_conversations(id TEXT PRIMARY KEY,kind TEXT NOT NULL CHECK(kind='direct'),persona_id TEXT NOT NULL UNIQUE REFERENCES personas(id),created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')));
        INSERT INTO old_conversations SELECT id,kind,persona_id,created_at FROM conversations;
        DROP TABLE conversations; ALTER TABLE old_conversations RENAME TO conversations;
        PRAGMA user_version=7;").unwrap();
}
