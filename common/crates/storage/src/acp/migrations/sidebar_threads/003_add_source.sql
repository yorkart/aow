ALTER TABLE sidebar_threads ADD COLUMN source TEXT NOT NULL DEFAULT 'user' CHECK (source IN ('user', 'cli'));
