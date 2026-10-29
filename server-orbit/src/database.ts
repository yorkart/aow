import { chmodSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

export interface Account {
  username: string;
  salt: string;
  password_hash: string;
  revision: string;
  active_org_id: string | null;
}

export interface Org {
  id: string;
  name: string;
  created_at: string;
}

/** Fresh databases only need table creation; no version migration framework. */
export function openDatabase(stateDir: string): DatabaseSync {
  mkdirSync(stateDir, { recursive: true, mode: 0o700 });
  const path = join(stateDir, 'orbit.sqlite');
  const database = new DatabaseSync(path);
  chmodSync(path, 0o600);
  database.exec(`
    PRAGMA foreign_keys = ON;
    PRAGMA journal_mode = WAL;
    PRAGMA busy_timeout = 5000;
    CREATE TABLE IF NOT EXISTS orgs (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80),
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS account (
      id INTEGER PRIMARY KEY CHECK(id = 1),
      username TEXT NOT NULL,
      salt TEXT NOT NULL,
      password_hash TEXT NOT NULL,
      revision TEXT NOT NULL,
      active_org_id TEXT REFERENCES orgs(id)
    );
    CREATE TABLE IF NOT EXISTS sessions (
      token_hash TEXT PRIMARY KEY,
      account_revision TEXT NOT NULL,
      created_at INTEGER NOT NULL,
      last_active INTEGER NOT NULL
    );
  `);
  return database;
}

export function account(database: DatabaseSync): Account | undefined {
  return database.prepare('SELECT * FROM account WHERE id = 1').get() as unknown as Account | undefined;
}

export function orgState(database: DatabaseSync) {
  return {
    orgs: database.prepare('SELECT id, name, created_at FROM orgs ORDER BY created_at, id').all() as unknown as Org[],
    active_org_id: account(database)?.active_org_id ?? null,
  };
}

export function transaction<T>(database: DatabaseSync, operation: () => T): T {
  database.exec('BEGIN IMMEDIATE');
  try {
    const value = operation();
    database.exec('COMMIT');
    return value;
  } catch (error) {
    database.exec('ROLLBACK');
    throw error;
  }
}
