ALTER TABLE sessions ADD COLUMN model_config TEXT;
ALTER TABLE sessions ADD COLUMN archived INTEGER DEFAULT 0;
ALTER TABLE messages ADD COLUMN active INTEGER DEFAULT 1;
ALTER TABLE messages ADD COLUMN compacted INTEGER DEFAULT 0;
UPDATE sessions SET archived = 1 WHERE id = 'root';
UPDATE messages SET active = 0;
UPDATE messages SET compacted = 1 WHERE id = 1;
