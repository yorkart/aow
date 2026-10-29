import { randomUUID } from 'node:crypto';
import { mkdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import type { DatabaseSync } from 'node:sqlite';
import { transaction } from '../database.js';
import { HttpError } from '../errors.js';
import { ConnectorAdapters, apiCredentials, type AdapterOptions } from './adapters.js';
import { ConnectorAuthFlows } from './auth-flows.js';
import { isolatedEnvironment } from './process.js';
import { sourceRoot, writeCredentials } from './identity.js';
import type { CheckResult, ConnectorRow, Identity } from './types.js';
import { CredentialVault } from './vault.js';

function shortText(value: unknown, field: string, limit: number, required = false): string {
  if (typeof value !== 'string' || value.length > limit || /\p{Cc}/u.test(value) || required && !value.trim()) throw new HttpError(422, 'invalid_connector', `请填写有效的${field}。`);
  return value.trim();
}

export class Connectors {
  readonly adapters: ConnectorAdapters;
  readonly vault: CredentialVault;
  readonly authFlows: ConnectorAuthFlows;
  constructor(readonly database: DatabaseSync, readonly stateDir: string, options: AdapterOptions = {}) {
    this.adapters = new ConnectorAdapters(stateDir, options);
    this.vault = new CredentialVault(stateDir, database);
    this.authFlows = new ConnectorAuthFlows(database, this.adapters, this.vault);
  }

  checkOrg(org: string) {
    if (!this.database.prepare('SELECT id FROM orgs WHERE id = ?').get(org)) throw new HttpError(404, 'org_not_found', '组织不存在。');
  }

  row(org: string, id: string): ConnectorRow {
    this.checkOrg(org);
    const row = this.database.prepare('SELECT * FROM connectors WHERE org_id = ? AND id = ?').get(org, id) as unknown as ConnectorRow | undefined;
    if (!row) throw new HttpError(404, 'connector_not_found', '连接不存在。');
    return row;
  }

  public(row: ConnectorRow) {
    const agents = this.database.prepare('SELECT agent_id FROM agent_connectors WHERE org_id = ? AND connector_id = ? ORDER BY agent_id').all(row.org_id, row.id) as unknown as { agent_id: string }[];
    return {
      id: row.id, org_id: row.org_id, name: row.name, provider: row.provider, executor: row.executor, auth_method: row.auth_method,
      account_note: row.account_note, account_summary: row.account_summary, status: row.enabled ? row.status : 'disabled',
      status_message: row.status_message, enabled: Boolean(row.enabled), revision: row.revision,
      credential_configured: row.credentials !== null, checked_at: row.checked_at, updated_at: row.updated_at,
      models: JSON.parse(row.models_json) as string[], agents, agent_count: agents.length,
    };
  }

  get(org: string, id: string) { return this.public(this.row(org, id)); }
  list(org: string) {
    this.checkOrg(org);
    const rows = this.database.prepare('SELECT * FROM connectors WHERE org_id = ? ORDER BY updated_at DESC, id').all(org) as unknown as ConnectorRow[];
    return rows.map(row => this.public(row));
  }

  private stored(row: ConnectorRow): Identity {
    if (!row.credentials) throw new HttpError(422, 'authentication_required', '此连接尚未配置认证，请先完成登录或验证 API Key。');
    return { credentials: this.vault.open(row.credentials, `${row.org_id}:connector:${row.id}`), summary: row.account_summary };
  }

  private async identity(org: string, row: ConnectorRow, input: Record<string, unknown>, existing?: ConnectorRow): Promise<Identity> {
    if (row.auth_method === 'local') return this.adapters.local(row.executor);
    if (row.auth_method === 'api_key' && input.api_key !== undefined && input.api_key !== '') return { credentials: apiCredentials(input.api_key), summary: `${row.provider} · API Key` };
    if (row.auth_method === 'independent' && typeof input.auth_session_id === 'string') return this.authFlows.credentials(org, input.auth_session_id, row.provider, row.executor);
    if (existing && existing.provider === row.provider && existing.executor === row.executor && existing.auth_method === row.auth_method) return this.stored(existing);
    throw new HttpError(422, 'authentication_required', '请先完成认证；也可以先保存待认证的连接配置。');
  }

  private errorResult(error: unknown): CheckResult {
    if (error instanceof HttpError) return { status: ['local_identity_missing', 'authentication_required', 'invalid_api_key', 'invalid_auth_session'].includes(error.code) ? 'auth_required' : 'error', message: error.message };
    return { status: 'error', message: '无法检查连接，请检查本地执行器及认证配置。' };
  }

  private async candidate(org: string, input: Record<string, unknown>, previous?: ConnectorRow): Promise<ConnectorRow> {
    this.checkOrg(org);
    const selected = await this.adapters.validate(input.provider, input.executor, input.auth_method);
    return {
      id: previous?.id ?? randomUUID(), org_id: org,
      name: shortText(input.name ?? previous?.name ?? '连接检测', '连接名称', 100, true),
      provider: selected.provider, executor: selected.executor, auth_method: selected.method,
      account_note: shortText(input.account_note ?? previous?.account_note ?? '', '账号备注', 500),
      account_summary: '', source_root: selected.method === 'local' ? sourceRoot(selected.executor, this.adapters.localHome, this.adapters.env) : null,
      credentials: null, status: 'auth_required', status_message: '尚未验证，请完成认证并检查连接。', enabled: previous?.enabled ?? 1,
      revision: (previous?.revision ?? 0) + 1, models_json: '[]', checked_at: null, updated_at: new Date().toISOString(),
    };
  }

  async verify(org: string, input: Record<string, unknown>) {
    const previous = typeof input.connector_id === 'string' ? this.row(org, input.connector_id) : undefined;
    const row = await this.candidate(org, input, previous);
    try {
      const identity = await this.identity(org, row, input, previous);
      return await this.adapters.check(row.executor, row.provider, identity);
    } catch (error) { return this.errorResult(error); }
  }

  async save(org: string, input: Record<string, unknown>, id?: string) {
    const previous = id ? this.row(org, id) : undefined;
    if (previous && input.revision !== previous.revision) throw new HttpError(409, 'connector_conflict', '连接已被其他窗口更新，请重新打开后编辑。');
    const row = await this.candidate(org, input, previous);
    const same = previous && previous.provider === row.provider && previous.executor === row.executor && previous.auth_method === row.auth_method;
    const replacement = input.api_key !== undefined && input.api_key !== '' || typeof input.auth_session_id === 'string';
    if (same && !replacement) {
      row.credentials = previous.credentials; row.account_summary = previous.account_summary;
      row.status = previous.status; row.status_message = previous.status_message; row.checked_at = previous.checked_at; row.models_json = previous.models_json;
    }
    let identity: Identity | undefined;
    if (row.auth_method !== 'local' && (replacement || same && previous.credentials !== null || input.connect === true)) identity = await this.identity(org, row, input, previous);
    if (input.connect === true) {
      if (!row.enabled) throw new HttpError(409, 'connector_disabled', '请先启用此连接，再检查认证。');
      identity ??= await this.identity(org, row, input, previous);
      const result = await this.adapters.check(row.executor, row.provider, identity);
      if (result.status !== 'available') throw new HttpError(422, 'connection_failed', result.message);
      row.status = 'available'; row.status_message = result.message; row.account_summary = result.summary ?? identity.summary;
      row.models_json = JSON.stringify(result.models ?? []); row.checked_at = new Date().toISOString();
    } else if (identity?.credentials.kind === 'api_key' && replacement) {
      const result = await this.adapters.checkKey(row.provider, identity.credentials.key ?? '');
      if (result.status !== 'available') throw new HttpError(422, 'invalid_api_key', result.message);
      row.status = 'auth_required'; row.status_message = '密钥已验证，请保存并连接以检查执行器。';
      row.account_summary = result.summary ?? identity.summary; row.models_json = JSON.stringify(result.models ?? []);
    }
    if (identity && row.auth_method !== 'local') { row.credentials = this.vault.seal(identity.credentials, `${org}:connector:${row.id}`); row.account_summary ||= identity.summary; }
    if (row.auth_method === 'local') row.credentials = null;
    transaction(this.database, () => {
      if (previous) {
        const changed = this.database.prepare(`UPDATE connectors SET name = ?, provider = ?, executor = ?, auth_method = ?, account_note = ?, account_summary = ?, source_root = ?, credentials = ?, status = ?, status_message = ?, enabled = ?, revision = ?, models_json = ?, checked_at = ?, updated_at = ? WHERE org_id = ? AND id = ? AND revision = ?`)
          .run(row.name, row.provider, row.executor, row.auth_method, row.account_note, row.account_summary, row.source_root, row.credentials, row.status, row.status_message, row.enabled, row.revision, row.models_json, row.checked_at, row.updated_at, org, row.id, previous.revision);
        if (!changed.changes) throw new HttpError(409, 'connector_conflict', '连接已被更新，请重新打开。');
      } else this.database.prepare(`INSERT INTO connectors (id, org_id, name, provider, executor, auth_method, account_note, account_summary, source_root, credentials, status, status_message, enabled, revision, models_json, checked_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
        .run(row.id, org, row.name, row.provider, row.executor, row.auth_method, row.account_note, row.account_summary, row.source_root, row.credentials, row.status, row.status_message, row.enabled, row.revision, row.models_json, row.checked_at, row.updated_at);
      if (typeof input.auth_session_id === 'string' && row.auth_method === 'independent') {
        const consumed = this.database.prepare("UPDATE connector_auth_sessions SET credentials = NULL WHERE id = ? AND org_id = ? AND status = 'complete' AND credentials IS NOT NULL AND expires_at > ?").run(input.auth_session_id, org, Date.now());
        if (!consumed.changes) throw new HttpError(409, 'invalid_auth_session', '授权已失效，请重新登录。');
      }
    });
    return this.get(org, row.id);
  }

  async check(org: string, id: string) {
    const row = this.row(org, id);
    if (!row.enabled) throw new HttpError(409, 'connector_disabled', '请先启用此连接。');
    let identity: Identity | undefined; let result: CheckResult;
    try { identity = row.auth_method === 'local' ? await this.adapters.local(row.executor) : this.stored(row); result = await this.adapters.check(row.executor, row.provider, identity); }
    catch (error) { result = this.errorResult(error); }
    this.database.prepare('UPDATE connectors SET status = ?, status_message = ?, account_summary = ?, source_root = ?, models_json = ?, checked_at = ? WHERE org_id = ? AND id = ? AND revision = ?')
      .run(result.status, result.message, result.summary ?? identity?.summary ?? row.account_summary, identity?.sourceRoot ?? row.source_root, JSON.stringify(result.models ?? []), new Date().toISOString(), org, id, row.revision);
    return this.get(org, id);
  }

  enable(org: string, id: string, input: Record<string, unknown>) {
    const row = this.row(org, id);
    if (input.revision !== row.revision) throw new HttpError(409, 'connector_conflict', '连接已更新，请重新打开。');
    if (typeof input.enabled !== 'boolean') throw new HttpError(422, 'invalid_connector', '启用状态无效。');
    this.database.prepare('UPDATE connectors SET enabled = ?, status = ?, status_message = ?, revision = revision + 1, updated_at = ? WHERE org_id = ? AND id = ?')
      .run(input.enabled ? 1 : 0, input.enabled ? 'auth_required' : 'disabled', input.enabled ? '连接已启用，请重新检查认证。' : '连接已停用，新运行不能使用此连接。', new Date().toISOString(), org, id);
    return this.get(org, id);
  }

  remove(org: string, id: string, revision: unknown) {
    transaction(this.database, () => {
      const row = this.row(org, id);
      if (revision !== row.revision) throw new HttpError(409, 'connector_conflict', '连接已更新，请重新打开。');
      if (this.database.prepare('SELECT 1 FROM agent_connectors WHERE org_id = ? AND connector_id = ?').get(org, id)) throw new HttpError(409, 'connector_in_use', '请先在 Agent 配置中解除此连接的引用，再删除。');
      this.database.prepare('DELETE FROM connectors WHERE org_id = ? AND id = ?').run(org, id);
    });
  }

  setAgentConnector(org: string, agent: string, id: string) {
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$/.test(agent)) throw new HttpError(422, 'invalid_agent', 'Agent 标识无效。');
    const row = this.row(org, id);
    if (!row.enabled || row.status !== 'available') throw new HttpError(409, 'connector_unavailable', '请选择一个可用连接。');
    this.database.prepare('INSERT INTO agent_connectors VALUES (?, ?, ?) ON CONFLICT(org_id, agent_id) DO UPDATE SET connector_id = excluded.connector_id').run(org, agent, id);
  }

  /** Internal runtime API. Local credentials are freshly read for every new execution. */
  async prepareExecution(org: string, id: string, executionId: string) {
    const row = this.row(org, id);
    if (!row.enabled || row.status !== 'available') throw new HttpError(409, 'connector_unavailable', '此连接不可用，请先检查认证。');
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$/.test(executionId)) throw new HttpError(422, 'invalid_execution', '运行标识无效。');
    const identity = row.auth_method === 'local' ? await this.adapters.local(row.executor) : this.stored(row);
    const root = join(this.stateDir, 'connector-runs', org, id); await mkdir(root, { recursive: true, mode: 0o700 });
    const directory = join(root, executionId);
    try { await mkdir(directory, { mode: 0o700 }); }
    catch { throw new HttpError(409, 'execution_exists', '此隔离环境已存在，请使用新的执行标识。'); }
    const home = join(directory, 'home');
    try {
      const env = await isolatedEnvironment(home, this.adapters.env);
      await writeCredentials(home, identity.credentials, row.executor, row.provider, env);
      return { home, env, provider: row.provider, executor: row.executor, connector_id: row.id, revision: row.revision, account_summary: identity.summary };
    } catch (error) { await rm(directory, { recursive: true, force: true }); throw error; }
  }

  async close() { this.authFlows.close(); await this.adapters.close(); }
}
