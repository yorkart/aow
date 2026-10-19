INSERT INTO sessions (id, source, cwd, title, started_at)
VALUES ('other', 'cli', '/elsewhere', 'Other conversation', 1700000002);
INSERT INTO messages (session_id, role, content, timestamp, finish_reason) VALUES
    ('root', 'user', 'Second question', 1700000002.123456, NULL),
    ('root', 'assistant', 'Second answer', 1700000003.123456, 'stop'),
    ('other', 'user', 'Unrelated question', 1700000004.123456, NULL);
