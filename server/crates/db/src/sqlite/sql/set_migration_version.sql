INSERT INTO schema_migrations(domain, version) VALUES (?1, ?2)
ON CONFLICT(domain) DO UPDATE SET version = excluded.version
