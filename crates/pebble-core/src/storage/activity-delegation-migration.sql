-- Private activity tasks have durable native identities but no message trigger.
-- Rebuild only the three task tables so legacy user-task rows retain their
-- exact records while trigger_id stops requiring a fabricated message.
ALTER TABLE delegation_submissions RENAME TO delegation_submissions_v18;
ALTER TABLE delegation_runs RENAME TO delegation_runs_v18;
ALTER TABLE delegated_tasks RENAME TO delegated_tasks_v18;
CREATE TABLE delegated_tasks (
 id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL REFERENCES conversations(id),
 persona_id TEXT NOT NULL REFERENCES personas(id), trigger_id TEXT NOT NULL,
 request_key TEXT NOT NULL, record TEXT NOT NULL,
 origin TEXT NOT NULL DEFAULT 'user', delivery TEXT NOT NULL DEFAULT 'room_report',
 activity_id TEXT, activity_step_id TEXT,
 UNIQUE(conversation_id,trigger_id,request_key)
);
INSERT INTO delegated_tasks(id,conversation_id,persona_id,trigger_id,request_key,record,origin,delivery,activity_id,activity_step_id)
 SELECT id,conversation_id,persona_id,trigger_id,request_key,record,origin,delivery,activity_id,activity_step_id FROM delegated_tasks_v18;
CREATE TABLE delegation_submissions (
 owner_id TEXT NOT NULL REFERENCES model_calls(id), provider_call_id TEXT NOT NULL,
 task_id TEXT REFERENCES delegated_tasks(id), result TEXT NOT NULL,
 PRIMARY KEY(owner_id,provider_call_id)
);
INSERT INTO delegation_submissions SELECT * FROM delegation_submissions_v18;
CREATE TABLE delegation_runs(task_id TEXT NOT NULL REFERENCES delegated_tasks(id),generation INTEGER NOT NULL,record TEXT NOT NULL,PRIMARY KEY(task_id,generation));
INSERT INTO delegation_runs SELECT * FROM delegation_runs_v18;
DROP TABLE delegation_submissions_v18;
DROP TABLE delegation_runs_v18;
DROP TABLE delegated_tasks_v18;
CREATE TABLE activity_delegation_submissions (
 owner_id TEXT NOT NULL REFERENCES activity_calls(id), provider_call_id TEXT NOT NULL,
 task_id TEXT REFERENCES delegated_tasks(id), result TEXT NOT NULL,
 PRIMARY KEY(owner_id,provider_call_id)
);
ALTER TABLE model_sessions ADD COLUMN private_activity_contract INTEGER NOT NULL DEFAULT 0;
PRAGMA user_version=19;
