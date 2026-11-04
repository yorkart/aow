-- Preserve conversation policies, explicit notification history, and undelivered
-- messages while retiring the former execution mirror. Historic migration files
-- remain necessary for upgrades; no execution tables survive this migration.
CREATE TABLE task_conversation_policies (
    thread_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    payload TEXT NOT NULL
);
INSERT INTO task_conversation_policies
SELECT resource_id, task_id, json_object('workspace',workspace,'workspace_intent',workspace_intent,
    'target_task_id',target_task_id,'target_revision',target_revision)
FROM task_resources JOIN tasks ON tasks.id=task_resources.task_id WHERE kind='acp';
INSERT OR REPLACE INTO task_conversation_policies
SELECT thread_id,task_id,json_extract(payload,'$.execution') FROM task_runs
WHERE json_type(payload,'$.execution')='object' ORDER BY rowid;

CREATE TABLE task_events (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    payload TEXT NOT NULL,
    is_read INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    UNIQUE(thread_id,request_id)
);
CREATE INDEX task_events_order ON task_events(created_at DESC,id);
INSERT INTO task_events
SELECT i.id,json_extract(r.payload,'$.project_id'),r.thread_id,i.id,i.created_at,
    json_object('id',i.id,'request_id',i.id,'project_id',json_extract(r.payload,'$.project_id'),
      'task_id',r.task_id,'thread_id',r.thread_id,
      'kind',CASE WHEN r.state='result_available' THEN 'result' ELSE 'blocked' END,
      'message',COALESCE(json_extract(r.payload,'$.result'),json_extract(r.payload,'$.error'),json_extract(r.payload,'$.prompt'),''),
      'target',CASE WHEN json_type(r.payload,'$.source')='object' THEN json_object(
          'task_id',json_extract(r.payload,'$.source.task_id'),'thread_id',json_extract(r.payload,'$.source.thread_id')) ELSE NULL END,
      'source',json_remove(json_extract(r.payload,'$.source'),'$.parent_run_id'),
      'created_at',i.created_at),i.is_read,i.archived
FROM task_inbox i JOIN task_runs r ON r.id=i.run_id;

CREATE TABLE task_messages (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    event_id TEXT UNIQUE,
    state TEXT NOT NULL CHECK(state IN ('pending','delivered','blocked')),
    payload TEXT NOT NULL,
    UNIQUE(task_id,request_id)
);
CREATE INDEX task_messages_pending ON task_messages(state);
INSERT INTO task_messages
SELECT id,task_id,request_id,thread_id,NULL,'pending',json_object(
    'id',id,'task_id',task_id,'request_id',request_id,'thread_id',thread_id,
    'prompt',json_extract(payload,'$.prompt'),
    'source',json_remove(json_extract(payload,'$.source'),'$.parent_run_id'),
    'event_id',NULL,'state','pending','created_at',json_extract(payload,'$.created_at'),
    'attempts',0,'retry_at',0,'error',NULL)
FROM task_runs WHERE state IN ('queued','running') AND COALESCE(json_extract(payload,'$.trigger'),'message')!='wakeup';
INSERT INTO task_messages
SELECT w.prompt_id,json_extract(e.payload,'$.target.task_id'),'inbox-'||e.id,json_extract(e.payload,'$.target.thread_id'),e.id,
    CASE WHEN w.state='processed' THEN 'delivered' WHEN w.state='blocked' THEN 'blocked' ELSE 'pending' END,
    json_object('id',w.prompt_id,'task_id',json_extract(e.payload,'$.target.task_id'),
        'request_id','inbox-'||e.id,'thread_id',json_extract(e.payload,'$.target.thread_id'),
        'event_id',e.id,'prompt','','source',json_extract(e.payload,'$.source'),
        'state',CASE WHEN w.state='processed' THEN 'delivered' WHEN w.state='blocked' THEN 'blocked' ELSE 'pending' END,
        'created_at',e.created_at,'attempts',COALESCE(json_extract(w.payload,'$.attempts'),0),
        'retry_at',COALESCE(json_extract(w.payload,'$.retry_at'),0),'error',json_extract(w.payload,'$.error'))
FROM task_wakeups w JOIN task_events e ON e.id=w.id WHERE json_type(e.payload,'$.target')='object';
DROP TABLE task_wakeups;
DROP TABLE task_inbox;
DROP TABLE task_runs;
