UPDATE messages SET timestamp = ?1 WHERE id = last_insert_rowid()
