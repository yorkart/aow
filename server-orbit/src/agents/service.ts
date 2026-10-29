import { randomUUID } from 'node:crypto';
import { rm } from 'node:fs/promises';
import type { DatabaseSync } from 'node:sqlite';
import type { Connectors } from '../connectors/service.js';
import { transaction } from '../database.js';
import { HttpError } from '../errors.js';
import type { Skills } from '../skills/service.js';
import { runtimeOptions, validateRuntime } from './capabilities.js';
import { AgentInstructions } from './instructions.js';
import { defaultRuntime, type AgentRow, type AgentRuntime } from './types.js';
import { AgentVariables } from './variables.js';

type Section = 'identity' | 'runtime' | 'skills' | 'variables';
function text(value: unknown, label: string, limit: number, required = false) {
  if (typeof value !== 'string' || value.length > limit || /\p{Cc}/u.test(value.replace(/[\n\t]/g, '')) || required && !value.trim()) throw new HttpError(422, 'invalid_agent', `请填写有效的${label}。`);
  return value.trim();
}

export class Agents {
  readonly instructions: AgentInstructions;
  readonly variables: AgentVariables;
  constructor(readonly database: DatabaseSync, readonly stateDir: string, readonly connectors: Connectors, readonly skills: Skills) {
    this.instructions = new AgentInstructions(stateDir); this.variables = new AgentVariables(database, connectors.vault);
  }
  row(org: string, id: string): AgentRow {
    this.connectors.checkOrg(org);
    const row = this.database.prepare('SELECT * FROM agents WHERE org_id = ? AND id = ?').get(org, id) as unknown as AgentRow | undefined;
    if (!row) throw new HttpError(404, 'agent_not_found', 'Agent 不存在。');
    return row;
  }
  get(org: string, id: string) {
    const row = this.row(org, id);
    const ref = this.database.prepare('SELECT connector_id FROM agent_connectors WHERE org_id = ? AND agent_id = ?').get(org, id) as { connector_id: string };
    const connector = this.connectors.get(org, ref.connector_id);
    const ids = new Set((this.database.prepare('SELECT skill_id FROM agent_skills WHERE org_id = ? AND agent_id = ?').all(org, id) as { skill_id: string }[]).map(item => item.skill_id));
    const running = this.database.prepare("SELECT count(*) AS count FROM agent_runs WHERE org_id = ? AND agent_id = ? AND status = 'running'").get(org, id) as { count: number };
    const latest = this.database.prepare('SELECT status, created_at FROM agent_runs WHERE org_id = ? AND agent_id = ? ORDER BY created_at DESC LIMIT 1').get(org, id) as { status: string; created_at: string } | undefined;
    const unavailable = connector.status !== 'available';
    const status = !row.enabled ? 'disabled' : running.count ? 'running' : unavailable || latest?.status === 'error' ? 'error' : 'idle';
    return {
      id: row.id, org_id: org, name: row.name, role: row.role, description: row.description, enabled: Boolean(row.enabled),
      status, status_message: unavailable ? `Connector 不可用：${connector.status_message}` : status === 'error' ? '最近运行异常，请检查运行环境。' : status === 'disabled' ? '已停用，不接收新运行；已开始的运行可继续完成。' : status === 'running' ? '正在执行任务。' : '等待任务。',
      running_count: running.count, connector, runtime: JSON.parse(row.runtime_json) as AgentRuntime,
      revisions: { identity: row.identity_revision, runtime: row.runtime_revision, skills: row.skills_revision, variables: row.variables_revision },
      instructions_version_id: row.instructions_version_id, instructions_updated_at: row.instructions_updated_at,
      skills: this.skills.list(org).filter(skill => ids.has(skill.id)), variables: this.variables.public(org, id),
      last_run_at: latest?.created_at ?? null, recent_tasks: this.database.prepare('SELECT id, project_id, identifier, title, status, updated_at FROM tasks WHERE org_id = ? AND agent_id = ? ORDER BY updated_at DESC, id LIMIT 10').all(org, id) as { id: string; project_id: string; identifier: string; title: string; status: string; updated_at: string }[], created_at: row.created_at, updated_at: row.updated_at,
    };
  }
  list(org: string) {
    this.connectors.checkOrg(org);
    return (this.database.prepare('SELECT id FROM agents WHERE org_id = ? ORDER BY created_at, id').all(org) as { id: string }[]).map(item => this.get(org, item.id));
  }
  async create(org: string, input: Record<string, unknown>) {
    this.connectors.checkOrg(org);
    const name = text(input.name, '名称', 100, true); const role = text(input.role ?? '', '角色', 100); const description = text(input.description ?? '', '职责说明', 4000);
    const selected = await validateRuntime(this.connectors, org, input.connector_id, defaultRuntime);
    const id = randomUUID(); const prepared = await this.instructions.prepare(org, id, this.instructions.template(name, role, description));
    const now = new Date().toISOString();
    try { transaction(this.database, () => {
      this.database.prepare(`INSERT INTO agents (id, org_id, name, role, description, runtime_json, instructions_version_id, instructions_updated_at, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`).run(id, org, name, role, description, JSON.stringify(selected.runtime), prepared.version, now, now, now);
      this.connectors.setAgentConnector(org, id, selected.connector_id);
    }); } catch (error) { await rm(prepared.directory, { recursive: true, force: true }); throw error; }
    return this.get(org, id);
  }
  private update(org: string, id: string, section: Section, revision: unknown, operation: () => void) {
    transaction(this.database, () => {
      const previous = this.row(org, id);
      if (revision !== previous[`${section}_revision`]) throw new HttpError(409, 'agent_conflict', '此配置页已被其他窗口更新，请重新打开后编辑。');
      operation();
      this.database.prepare(`UPDATE agents SET ${section}_revision = ${section}_revision + 1, updated_at = ? WHERE org_id = ? AND id = ?`).run(new Date().toISOString(), org, id);
    });
    return this.get(org, id);
  }
  saveIdentity(org: string, id: string, input: Record<string, unknown>) {
    return this.update(org, id, 'identity', input.revision, () => {
      const row = this.row(org, id);
      if (input.enabled !== undefined && typeof input.enabled !== 'boolean') throw new HttpError(422, 'invalid_agent', '启用状态无效。');
      this.database.prepare('UPDATE agents SET name = ?, role = ?, description = ?, enabled = ? WHERE org_id = ? AND id = ?').run(text(input.name ?? row.name, '名称', 100, true), text(input.role ?? row.role, '角色', 100), text(input.description ?? row.description, '职责说明', 4000), input.enabled === undefined ? row.enabled : input.enabled ? 1 : 0, org, id);
    });
  }
  async saveRuntime(org: string, id: string, input: Record<string, unknown>) {
    this.row(org, id);
    const selected = await validateRuntime(this.connectors, org, input.connector_id, input.runtime);
    return this.update(org, id, 'runtime', input.revision, () => {
      this.connectors.setAgentConnector(org, id, selected.connector_id);
      this.database.prepare('UPDATE agents SET runtime_json = ? WHERE org_id = ? AND id = ?').run(JSON.stringify(selected.runtime), org, id);
    });
  }
  saveSkills(org: string, id: string, input: Record<string, unknown>) {
    return this.update(org, id, 'skills', input.revision, () => {
      if (!Array.isArray(input.skill_ids) || input.skill_ids.length > 100 || input.skill_ids.some(item => typeof item !== 'string') || new Set(input.skill_ids).size !== input.skill_ids.length) throw new HttpError(422, 'invalid_references', '技能选择无效。');
      for (const skill of input.skill_ids) this.skills.get(org, skill);
      this.database.prepare('DELETE FROM agent_skills WHERE org_id = ? AND agent_id = ?').run(org, id);
      for (const skill of input.skill_ids) this.database.prepare('INSERT INTO agent_skills VALUES (?, ?, ?)').run(org, id, skill);
    });
  }
  saveVariables(org: string, id: string, input: Record<string, unknown>) { return this.update(org, id, 'variables', input.revision, () => this.variables.replace(org, id, input.variables)); }
  async instructionFiles(org: string, id: string) { const row = this.row(org, id); return { version_id: row.instructions_version_id, files: await this.instructions.read(org, id, row.instructions_version_id) }; }
  async saveInstructions(org: string, id: string, input: Record<string, unknown>) {
    const row = this.row(org, id);
    if (input.base_version_id !== row.instructions_version_id) throw new HttpError(409, 'agent_conflict', '指令已被更新，请重新打开后编辑。');
    const prepared = await this.instructions.prepare(org, id, input.files);
    try {
      const now = new Date().toISOString();
      const updated = this.database.prepare('UPDATE agents SET instructions_version_id = ?, instructions_updated_at = ?, updated_at = ? WHERE org_id = ? AND id = ? AND instructions_version_id = ?').run(prepared.version, now, now, org, id, row.instructions_version_id);
      if (!updated.changes) throw new HttpError(409, 'agent_conflict', '指令已被更新，请重新打开后编辑。');
    } catch (error) { await rm(prepared.directory, { recursive: true, force: true }); throw error; }
    return this.get(org, id);
  }
  runtimeOptions(org: string, connector: string) { return runtimeOptions(this.connectors, org, connector); }
  async checkRuntime(org: string, id: string, input: Record<string, unknown>) {
    const agent = this.get(org, id); const connectorId = typeof input.connector_id === 'string' ? input.connector_id : agent.connector.id;
    const connector = await this.connectors.check(org, connectorId);
    if (connector.status !== 'available') return { status: 'error', message: connector.status_message, connector };
    await validateRuntime(this.connectors, org, connectorId, input.runtime ?? agent.runtime);
    return { status: 'available', message: '执行器、Connector 和运行配置检查通过。', connector };
  }
}
