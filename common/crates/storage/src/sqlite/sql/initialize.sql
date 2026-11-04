PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;
CREATE TABLE IF NOT EXISTS schema_migrations(domain TEXT PRIMARY KEY NOT NULL, version INTEGER NOT NULL) STRICT;
