ALTER TABLE model_sessions ADD COLUMN image_generation_tools INTEGER NOT NULL DEFAULT 0;
CREATE TABLE image_generations (
 id TEXT PRIMARY KEY,
 record TEXT NOT NULL
);
CREATE TABLE image_generation_submissions (
 owner_id TEXT NOT NULL REFERENCES model_calls(id),
 provider_call_id TEXT NOT NULL,
 operation_id TEXT REFERENCES image_generations(id),
 result TEXT NOT NULL,
 PRIMARY KEY(owner_id, provider_call_id)
);
PRAGMA user_version=14;
