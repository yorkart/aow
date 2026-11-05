CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT, cwd TEXT, title TEXT,
    started_at REAL, ended_at REAL, end_reason TEXT, parent_session_id TEXT);
CREATE TABLE messages (id INTEGER PRIMARY KEY, session_id TEXT, role TEXT,
    content TEXT, timestamp REAL);
