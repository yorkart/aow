import { randomUUID } from 'node:crypto';
import type { DatabaseSync } from 'node:sqlite';
import type { CredentialVault } from '../connectors/vault.js';
import { managedVariable } from '../connectors/process.js';
import { HttpError } from '../errors.js';
import type { VariableRow } from './types.js';

export class AgentVariables {
  constructor(private database: DatabaseSync, private vault: CredentialVault) {}
  rows(org: string, agent: string) { return this.database.prepare('SELECT * FROM agent_variables WHERE org_id = ? AND agent_id = ? ORDER BY name').all(org, agent) as unknown as VariableRow[]; }
  public(org: string, agent: string) { return this.rows(org, agent).map(row => ({ id: row.id, name: row.name, kind: row.kind, ...(row.kind === 'variable' ? { value: row.value! } : { configured: true }) })); }
  /** Called inside the Agent section's revision transaction. */
  replace(org: string, agent: string, input: unknown) {
    if (!Array.isArray(input) || input.length > 100) throw new HttpError(422, 'invalid_variables', '最多配置 100 个变量。');
    const previous = this.rows(org, agent); const names = new Set<string>(); const ids = new Set<string>(); let size = 0;
    const rows = input.map((item: unknown) => {
      if (!item || typeof item !== 'object') throw new HttpError(422, 'invalid_variables', '变量格式无效。');
      const value = item as Record<string, unknown>; const name = value.name;
      if (typeof name !== 'string' || !/^[A-Za-z_][A-Za-z0-9_]{0,79}$/.test(name) || names.has(name) || !['variable', 'secret'].includes(String(value.kind))) throw new HttpError(422, 'invalid_variables', '变量名称须唯一，并且只包含字母、数字和下划线。');
      if (managedVariable(name)) throw new HttpError(422, 'managed_variable', '不能覆盖 AI 认证或 Orbit 管理的运行环境字段。');
      names.add(name);
      const old = typeof value.id === 'string' ? previous.find(row => row.id === value.id) : undefined;
      if (value.id !== undefined && !old || old && ids.has(old.id)) throw new HttpError(422, 'invalid_variables', '变量引用无效，请重新打开配置。');
      const id = old?.id ?? randomUUID(); ids.add(id);
      if (value.value !== undefined && (typeof value.value !== 'string' || value.value.includes('\0') || Buffer.byteLength(value.value) > 64 * 1024)) throw new HttpError(422, 'invalid_variables', '变量值须为文本，且不超过 64 KiB。');
      if (value.value === undefined && value.kind === 'secret' && old?.kind === 'secret') return { id, name, kind: 'secret', value: null, secret: old.secret_id! };
      if (typeof value.value !== 'string' || value.kind === 'secret' && !value.value) throw new HttpError(422, 'secret_required', '请为新增或替换的 Secret 填写值。');
      size += Buffer.byteLength(value.value); if (size > 256 * 1024) throw new HttpError(422, 'invalid_variables', '变量总大小不能超过 256 KiB。');
      if (value.kind === 'variable') return { id, name, kind: 'variable', value: value.value, secret: null };
      const secret = randomUUID();
      this.database.prepare('INSERT INTO agent_secrets VALUES (?, ?, ?, ?)').run(secret, org, agent, this.vault.seal({ value: value.value }, `${org}:agent:${agent}:secret:${secret}`));
      return { id, name, kind: 'secret', value: null, secret };
    });
    let total = 0;
    for (const row of rows) {
      const secret = row.secret ? this.database.prepare('SELECT ciphertext FROM agent_secrets WHERE org_id = ? AND agent_id = ? AND id = ?').get(org, agent, row.secret) as { ciphertext: string } : undefined;
      total += Buffer.byteLength(row.value ?? this.vault.open<{ value: string }>(secret!.ciphertext, `${org}:agent:${agent}:secret:${row.secret}`).value);
      if (total > 256 * 1024) throw new HttpError(422, 'invalid_variables', '变量总大小不能超过 256 KiB。');
    }
    this.database.prepare('DELETE FROM agent_variables WHERE org_id = ? AND agent_id = ?').run(org, agent);
    for (const row of rows) this.database.prepare('INSERT INTO agent_variables VALUES (?, ?, ?, ?, ?, ?, ?)').run(row.id, org, agent, row.name, row.kind, row.value, row.secret);
    this.database.prepare('DELETE FROM agent_secrets WHERE org_id = ? AND agent_id = ? AND id NOT IN (SELECT secret_id FROM agent_variables WHERE org_id = ? AND agent_id = ? AND secret_id IS NOT NULL)').run(org, agent, org, agent);
  }
  /** Secrets are only decrypted for an internal execution snapshot. */
  environment(org: string, agent: string) {
    return Object.fromEntries(this.rows(org, agent).map(row => {
      const secret = row.secret_id ? this.database.prepare('SELECT ciphertext FROM agent_secrets WHERE org_id = ? AND agent_id = ? AND id = ?').get(org, agent, row.secret_id) as { ciphertext: string } : undefined;
      return [row.name, row.kind === 'variable' ? row.value! : this.vault.open<{ value: string }>(secret!.ciphertext, `${org}:agent:${agent}:secret:${row.secret_id}`).value];
    }));
  }
}
