//! Restore the actual v0.1.2 schema before exercising older-version upgrades.
pub fn v7(root: &std::path::Path) {
    v10(root);
    let db = rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF;
        DROP TABLE group_reply_slots; DROP TABLE group_rounds;
        ALTER TABLE model_calls DROP COLUMN slot_id;
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

/// Remove only v0.1.5 additions, preserving the populated v0.1.4 decisions.
#[allow(dead_code)]
pub fn v9(root: &std::path::Path) {
    v10(root);
    let db = rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap();
    db.execute_batch(
        "PRAGMA foreign_keys=OFF;
      DROP TABLE group_reply_slots; DROP TABLE group_rounds;
      ALTER TABLE model_calls DROP COLUMN slot_id;
      ALTER TABLE participation_decisions DROP COLUMN generation;
      ALTER TABLE participation_decisions DROP COLUMN phase;
      ALTER TABLE participation_decisions DROP COLUMN knowledge_version;
      ALTER TABLE participation_decisions DROP COLUMN predecessor_version;
      PRAGMA user_version=9;",
    )
    .unwrap();
}

#[allow(dead_code)]
pub fn v10(root: &std::path::Path) {
    let db = rusqlite::Connection::open(root.join("data/app.sqlite")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE image_generation_submissions; DROP TABLE image_generations; ALTER TABLE model_sessions DROP COLUMN image_generation_tools; DROP TABLE delegation_runs; DROP TABLE delegation_submissions; DROP TABLE delegated_tasks; DROP TABLE delegation_assignments; ALTER TABLE model_sessions DROP COLUMN delegation_tools; ALTER TABLE messages DROP COLUMN separate_delegations; DROP TABLE message_images; DROP TABLE image_assets; DROP TABLE context_marker_rooms; DROP TABLE context_markers; DROP TABLE memory_operations; DROP TABLE context_maintenance; ALTER TABLE model_sessions DROP COLUMN memory_tools; ALTER TABLE model_calls DROP COLUMN memory_snapshot; ALTER TABLE model_calls DROP COLUMN native_tool_output; PRAGMA user_version=10;").unwrap();
}
