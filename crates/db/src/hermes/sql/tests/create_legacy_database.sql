PRAGMA journal_mode=WAL;
CREATE TABLE sessions (
    id TEXT PRIMARY KEY, source TEXT, cwd TEXT, title TEXT,
    started_at REAL, ended_at REAL, end_reason TEXT, parent_session_id TEXT
);
CREATE TABLE messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT, role TEXT,
    content TEXT, tool_call_id TEXT, tool_calls TEXT, timestamp REAL, finish_reason TEXT
);
INSERT INTO sessions (id, source, cwd, title, started_at)
VALUES ('root', 'cli', '/repo', 'Native title', 1700000000);
INSERT INTO messages (session_id, role, content, timestamp, finish_reason) VALUES
    ('root', 'user', 'First question', 1700000000.123456, NULL),
    ('root', 'assistant', 'First answer', 1700000001.123456, 'stop');
