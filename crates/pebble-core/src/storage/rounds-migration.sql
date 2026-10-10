CREATE TABLE group_rounds (
    id TEXT PRIMARY KEY REFERENCES messages(id),
    conversation_id TEXT NOT NULL REFERENCES conversations(id),
    revision INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','superseded','completed','cancelled','interrupted')),
    continuation INTEGER NOT NULL DEFAULT 1 CHECK(continuation IN (0,1)),
    followups_used INTEGER NOT NULL DEFAULT 0 CHECK(followups_used BETWEEN 0 AND 2),
    followup_trigger TEXT REFERENCES messages(id),
    fallback_stage TEXT NOT NULL DEFAULT 'none' CHECK(fallback_stage IN ('none','intent','permission','reply','done')),
    last_reply TEXT REFERENCES messages(id)
);
CREATE TABLE group_reply_slots (
    id TEXT PRIMARY KEY,
    round_id TEXT NOT NULL REFERENCES group_rounds(id),
    decision_id TEXT NOT NULL UNIQUE REFERENCES participation_decisions(id),
    ordinal INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('required','optional','followup','fallback')),
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','admitted','completed','failed','cancelled','interrupted','declined')),
    UNIQUE(round_id,ordinal)
);
ALTER TABLE model_calls ADD COLUMN slot_id TEXT REFERENCES group_reply_slots(id);
ALTER TABLE participation_decisions ADD COLUMN generation INTEGER NOT NULL DEFAULT 0;
ALTER TABLE participation_decisions ADD COLUMN phase TEXT NOT NULL DEFAULT 'participation';
ALTER TABLE participation_decisions ADD COLUMN knowledge_version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE participation_decisions ADD COLUMN predecessor_version INTEGER NOT NULL DEFAULT 0;
CREATE INDEX round_room ON group_rounds(conversation_id,status);
CREATE INDEX slot_round ON group_reply_slots(round_id,status);
PRAGMA user_version=10;
