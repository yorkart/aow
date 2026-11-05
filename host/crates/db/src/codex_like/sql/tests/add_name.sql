ALTER TABLE threads ADD COLUMN name TEXT;
UPDATE threads SET name = 'Native first' WHERE id = 'first';
