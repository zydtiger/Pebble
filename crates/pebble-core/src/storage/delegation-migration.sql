ALTER TABLE model_sessions ADD COLUMN delegation_tools INTEGER NOT NULL DEFAULT 0;
CREATE TABLE delegated_tasks (
 id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL REFERENCES conversations(id),
 persona_id TEXT NOT NULL REFERENCES personas(id), trigger_id TEXT NOT NULL REFERENCES messages(id),
 request_key TEXT NOT NULL, record TEXT NOT NULL,
 UNIQUE(conversation_id,trigger_id,request_key)
);
CREATE TABLE delegation_submissions (
 owner_id TEXT NOT NULL REFERENCES model_calls(id), provider_call_id TEXT NOT NULL,
 task_id TEXT REFERENCES delegated_tasks(id), result TEXT NOT NULL,
 PRIMARY KEY(owner_id,provider_call_id)
);
ALTER TABLE messages ADD COLUMN separate_delegations INTEGER NOT NULL DEFAULT 0;
CREATE TABLE delegation_assignments(message_id TEXT NOT NULL REFERENCES messages(id),persona_id TEXT NOT NULL REFERENCES personas(id),PRIMARY KEY(message_id,persona_id));
CREATE TABLE delegation_runs(task_id TEXT NOT NULL REFERENCES delegated_tasks(id),generation INTEGER NOT NULL,record TEXT NOT NULL,PRIMARY KEY(task_id,generation));
PRAGMA user_version=13;
