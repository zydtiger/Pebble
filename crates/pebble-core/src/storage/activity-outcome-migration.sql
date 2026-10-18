ALTER TABLE activity_calls ADD COLUMN outcome_ids TEXT NOT NULL DEFAULT '[]';
PRAGMA user_version=20;
