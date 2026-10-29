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
    CREATE TABLE IF NOT EXISTS projects (
      id TEXT PRIMARY KEY,
      org_id TEXT NOT NULL REFERENCES orgs(id),
      name TEXT NOT NULL,
      directory TEXT NOT NULL,
      UNIQUE(org_id, directory),
      UNIQUE(org_id, id)
    );
    CREATE TABLE IF NOT EXISTS skills (
      id TEXT PRIMARY KEY,
      org_id TEXT NOT NULL REFERENCES orgs(id),
      name TEXT NOT NULL,
      description TEXT NOT NULL,
      current_version_id TEXT,
      source_project_id TEXT,
      source_path TEXT,
      updated_at TEXT NOT NULL,
      deleted_at TEXT,
      UNIQUE(org_id, id),
      FOREIGN KEY(org_id, source_project_id) REFERENCES projects(org_id, id)
    );
    CREATE UNIQUE INDEX IF NOT EXISTS skills_active_name ON skills(org_id, name COLLATE NOCASE) WHERE deleted_at IS NULL;
    CREATE TABLE IF NOT EXISTS skill_versions (
      id TEXT PRIMARY KEY,
      skill_id TEXT NOT NULL REFERENCES skills(id),
      number INTEGER NOT NULL,
      directory TEXT NOT NULL,
      content_hash TEXT NOT NULL,
      created_at TEXT NOT NULL,
      UNIQUE(skill_id, number),
      UNIQUE(skill_id, id)
    );
    CREATE TABLE IF NOT EXISTS agent_skills (
      org_id TEXT NOT NULL,
      agent_id TEXT NOT NULL,
      skill_id TEXT NOT NULL,
      PRIMARY KEY(org_id, agent_id, skill_id),
      FOREIGN KEY(org_id, skill_id) REFERENCES skills(org_id, id)
    );
    CREATE TABLE IF NOT EXISTS skill_runs (
      org_id TEXT NOT NULL REFERENCES orgs(id),
      agent_id TEXT NOT NULL,
      run_id TEXT NOT NULL,
      directory TEXT NOT NULL,
      created_at TEXT NOT NULL,
      PRIMARY KEY(org_id, agent_id, run_id)
    );
    CREATE TABLE IF NOT EXISTS skill_run_versions (
      org_id TEXT NOT NULL,
      agent_id TEXT NOT NULL,
      run_id TEXT NOT NULL,
      skill_id TEXT NOT NULL,
      version_id TEXT NOT NULL,
      name TEXT NOT NULL,
      PRIMARY KEY(org_id, agent_id, run_id, skill_id),
      FOREIGN KEY(org_id, agent_id, run_id) REFERENCES skill_runs(org_id, agent_id, run_id),
      FOREIGN KEY(org_id, skill_id) REFERENCES skills(org_id, id),
      FOREIGN KEY(skill_id, version_id) REFERENCES skill_versions(skill_id, id)
    );
    CREATE TABLE IF NOT EXISTS connectors (
      id TEXT PRIMARY KEY,
      org_id TEXT NOT NULL REFERENCES orgs(id),
      name TEXT NOT NULL,
      provider TEXT NOT NULL,
      executor TEXT NOT NULL,
      auth_method TEXT NOT NULL,
      account_note TEXT NOT NULL DEFAULT '',
      account_summary TEXT NOT NULL DEFAULT '',
      source_root TEXT,
      credentials TEXT,
      status TEXT NOT NULL DEFAULT 'auth_required',
      status_message TEXT NOT NULL DEFAULT '',
      enabled INTEGER NOT NULL DEFAULT 1,
      revision INTEGER NOT NULL DEFAULT 1,
      models_json TEXT NOT NULL DEFAULT '[]',
      checked_at TEXT,
      updated_at TEXT NOT NULL,
      UNIQUE(org_id, id)
    );
    CREATE TABLE IF NOT EXISTS agent_connectors (
      org_id TEXT NOT NULL,
      agent_id TEXT NOT NULL,
      connector_id TEXT NOT NULL,
      PRIMARY KEY(org_id, agent_id),
      FOREIGN KEY(org_id, connector_id) REFERENCES connectors(org_id, id)
    );
    CREATE TABLE IF NOT EXISTS connector_auth_sessions (
      id TEXT PRIMARY KEY,
      org_id TEXT NOT NULL REFERENCES orgs(id),
      provider TEXT NOT NULL,
      executor TEXT NOT NULL,
      status TEXT NOT NULL,
      verification_uri TEXT,
      user_code TEXT,
      message TEXT NOT NULL,
      credentials TEXT,
      account_summary TEXT NOT NULL DEFAULT '',
      expires_at INTEGER NOT NULL,
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS agents (
      id TEXT PRIMARY KEY,
      org_id TEXT NOT NULL REFERENCES orgs(id),
      name TEXT NOT NULL,
      role TEXT NOT NULL DEFAULT '',
      description TEXT NOT NULL DEFAULT '',
      enabled INTEGER NOT NULL DEFAULT 1,
      runtime_json TEXT NOT NULL,
      instructions_version_id TEXT NOT NULL,
      instructions_updated_at TEXT NOT NULL,
      identity_revision INTEGER NOT NULL DEFAULT 1,
      runtime_revision INTEGER NOT NULL DEFAULT 1,
      skills_revision INTEGER NOT NULL DEFAULT 1,
      variables_revision INTEGER NOT NULL DEFAULT 1,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      UNIQUE(org_id, id)
    );
    CREATE TABLE IF NOT EXISTS agent_secrets (
      id TEXT PRIMARY KEY,
      org_id TEXT NOT NULL,
      agent_id TEXT NOT NULL,
      ciphertext TEXT NOT NULL,
      UNIQUE(org_id, agent_id, id),
      FOREIGN KEY(org_id, agent_id) REFERENCES agents(org_id, id)
    );
    CREATE TABLE IF NOT EXISTS agent_variables (
      id TEXT PRIMARY KEY,
      org_id TEXT NOT NULL,
      agent_id TEXT NOT NULL,
      name TEXT NOT NULL,
      kind TEXT NOT NULL CHECK(kind IN ('variable', 'secret')),
      value TEXT,
      secret_id TEXT,
      UNIQUE(org_id, agent_id, name),
      FOREIGN KEY(org_id, agent_id) REFERENCES agents(org_id, id),
      FOREIGN KEY(org_id, agent_id, secret_id) REFERENCES agent_secrets(org_id, agent_id, id),
      CHECK((kind = 'variable' AND value IS NOT NULL AND secret_id IS NULL) OR (kind = 'secret' AND value IS NULL AND secret_id IS NOT NULL))
    );
    CREATE TABLE IF NOT EXISTS agent_runs (
      org_id TEXT NOT NULL,
      agent_id TEXT NOT NULL,
      run_id TEXT NOT NULL,
      project_id TEXT NOT NULL,
      status TEXT NOT NULL CHECK(status IN ('prepared', 'running', 'complete', 'error')),
      directory TEXT NOT NULL,
      snapshot_json TEXT NOT NULL,
      created_at TEXT NOT NULL,
      finished_at TEXT,
      PRIMARY KEY(org_id, agent_id, run_id),
      FOREIGN KEY(org_id, agent_id) REFERENCES agents(org_id, id),
      FOREIGN KEY(org_id, project_id) REFERENCES projects(org_id, id)
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
