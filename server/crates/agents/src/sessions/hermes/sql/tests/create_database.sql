PRAGMA journal_mode=WAL;
CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT, cwd TEXT, title TEXT,
    started_at REAL, ended_at REAL, end_reason TEXT, parent_session_id TEXT, model_config TEXT, archived INTEGER DEFAULT 0);
CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT, role TEXT,
    content TEXT, tool_call_id TEXT, tool_calls TEXT, timestamp REAL, finish_reason TEXT,
    reasoning TEXT, active INTEGER DEFAULT 1, compacted INTEGER DEFAULT 0);
