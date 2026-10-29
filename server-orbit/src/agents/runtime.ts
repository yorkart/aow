import { mkdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { requireExecutor } from '../connectors/process.js';
import { HttpError } from '../errors.js';
import { readPackage, writePackage } from '../skills/files.js';
import { SkillReferences } from '../skills/references.js';
import type { Agents } from './service.js';
import { validateRuntime } from './capabilities.js';
import { Projects } from '../projects/service.js';
import { Workspaces } from '../projects/workspaces.js';
import { Tasks } from '../projects/tasks.js';
import { TaskWorkspaces } from '../projects/task-workspaces.js';

/** Internal integration point for the future task runner; never exposed over HTTP. */
export class AgentRuns {
  private references: SkillReferences;
  constructor(private agents: Agents) { this.references = new SkillReferences(agents.skills); }

  async prepare(org: string, agentId: string, runId: string, projectId: string, workspaceId?: string, taskId?: string) {
    if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$/.test(runId)) throw new HttpError(422, 'invalid_execution', '运行标识无效。');
    const agent = this.agents.get(org, agentId);
    if (this.agents.database.prepare('SELECT 1 FROM agent_runs WHERE org_id = ? AND agent_id = ? AND run_id = ?').get(org, agentId, runId)) throw new HttpError(409, 'execution_exists', '此运行已经存在，请使用新的运行标识。');
    if (!agent.enabled) throw new HttpError(409, 'agent_disabled', '此 Agent 已停用，不能接收新运行。');
    if (agent.connector.status !== 'available') throw new HttpError(409, 'connector_unavailable', 'Agent 的 Connector 不可用。');
    const projects = new Projects(this.agents.database, this.agents.stateDir); const workspaces = new Workspaces(projects); projects.row(org, projectId);
    // Capture editable Agent values before the first await. Later changes affect later runs.
    const variables = this.agents.variables.environment(org, agentId);
    await validateRuntime(this.agents.connectors, org, agent.connector.id, agent.runtime);
    const workspace = await workspaces.resolve(org, projectId, workspaceId);
    if (taskId) { const task = new Tasks(projects, workspaces).row(org, taskId); if (task.project_id !== projectId || task.workspace_id !== workspace.id || task.agent_id !== agentId) throw new HttpError(409, 'task_conflict', '任务的项目、工作区或负责 Agent 已改变，请重新准备运行。'); }
    const snapshot = { agent_id: agentId, name: agent.name, role: agent.role, description: agent.description, revisions: agent.revisions, instructions_version_id: agent.instructions_version_id, connector_id: agent.connector.id, connector_revision: agent.connector.revision, provider: agent.connector.provider, executor: agent.connector.executor, runtime: agent.runtime, skills: agent.skills.map(skill => ({ skill_id: skill.id, version_id: skill.current_version_id, name: skill.name, version: skill.version })), variables: agent.variables, project_id: projectId, workspace_id: workspace.id, task_id: taskId ?? null };
    const cwd = workspace.directory;
    const command = await requireExecutor(agent.connector.executor, this.agents.connectors.adapters.env);
    const directory = join(this.agents.stateDir, 'agents', org, agentId, 'runs', runId);
    const execution = await this.agents.connectors.prepareExecution(org, agent.connector.id, runId, directory);
    try {
      // Connector edits during identity preparation must not silently change the pinned runtime.
      if (execution.revision !== agent.connector.revision) throw new HttpError(409, 'connector_conflict', 'Connector 已改变，请重新准备运行。');
      const instructions = await this.agents.instructions.read(org, agentId, agent.instructions_version_id);
      const nativeRoot = join(execution.home, agent.connector.executor === 'codex' ? '.codex' : agent.connector.executor === 'claude' ? '.claude' : '.gemini');
      const instructionRoot = join(nativeRoot, 'orbit-instructions');
      await writePackage(instructionRoot, instructions);
      const entry = instructions.find(file => file.kind === 'file' && file.path === 'AGENTS.md');
      const nativeEntry = agent.connector.executor === 'codex' ? 'AGENTS.md' : agent.connector.executor === 'claude' ? 'CLAUDE.md' : 'GEMINI.md';
      const content = agent.connector.executor === 'codex' && entry?.kind === 'file' ? `Agent 辅助指令文件目录：${instructionRoot}\n\n${entry.content}` : '@orbit-instructions/AGENTS.md\n';
      await writeFile(join(nativeRoot, nativeEntry), content, { flag: 'wx', mode: 0o400 });
      const pinned = await this.references.pinRun(org, agentId, runId, agent.skills);
      const skillsRoot = agent.connector.executor === 'codex' ? join(execution.home, '.agents/skills') : join(nativeRoot, 'skills');
      await mkdir(skillsRoot, { recursive: true, mode: 0o700 });
      for (const skill of pinned.skills) await writePackage(join(skillsRoot, skill.name), await readPackage(skill.directory));
      const args = agent.connector.executor === 'codex' ? ['exec', '--json'] : agent.connector.executor === 'claude' ? ['--print', '--output-format', 'stream-json', '--verbose'] : ['--output-format', 'stream-json'];
      if (agent.runtime.model) args.push('--model', agent.runtime.model);
      if (agent.runtime.reasoning_effort) args.push('-c', `model_reasoning_effort=${JSON.stringify(agent.runtime.reasoning_effort)}`);
      Object.assign(execution.env, variables);
      if (Buffer.byteLength(JSON.stringify(variables)) > 256 * 1024) throw new HttpError(422, 'invalid_variables', '变量总大小不能超过 256 KiB。');
      await writeFile(join(directory, 'snapshot.json'), JSON.stringify(snapshot, null, 2), { mode: 0o600, flag: 'wx' });
      this.agents.database.prepare('INSERT INTO agent_runs (org_id, agent_id, run_id, project_id, status, directory, snapshot_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)').run(org, agentId, runId, projectId, 'prepared', directory, JSON.stringify(snapshot), new Date().toISOString());
      workspaces.used(org, projectId, workspace.id);
      return { ...execution, cwd, command, args, timeout_seconds: agent.runtime.timeout_seconds, run_id: runId, snapshot };
    } catch (error) {
      const pinned = this.agents.database.prepare('SELECT directory FROM skill_runs WHERE org_id = ? AND agent_id = ? AND run_id = ?').get(org, agentId, runId) as { directory: string } | undefined;
      this.agents.database.prepare('DELETE FROM skill_run_versions WHERE org_id = ? AND agent_id = ? AND run_id = ?').run(org, agentId, runId);
      this.agents.database.prepare('DELETE FROM skill_runs WHERE org_id = ? AND agent_id = ? AND run_id = ?').run(org, agentId, runId);
      if (pinned) await rm(pinned.directory, { recursive: true, force: true });
      await rm(directory, { recursive: true, force: true }); throw error;
    }
  }
  async prepareForTask(org: string, taskId: string, runId: string, isolated = true) {
    const projects = new Projects(this.agents.database, this.agents.stateDir); const tasks = new Tasks(projects, new Workspaces(projects)); const task = tasks.get(org, taskId);
    if (!task.agent_id) throw new HttpError(422, 'invalid_task_agent', '请先为任务选择负责 Agent。');
    const agent = this.agents.get(org, task.agent_id);
    if (!agent.enabled) throw new HttpError(409, 'agent_disabled', '此 Agent 已停用，不能接收新运行。');
    await validateRuntime(this.agents.connectors, org, agent.connector.id, agent.runtime);
    const workspace = await new TaskWorkspaces(tasks).prepare(org, taskId, isolated);
    return this.prepare(org, task.agent_id, runId, task.project_id, workspace.id, taskId);
  }
  start(org: string, agent: string, run: string) {
    this.agents.row(org, agent);
    const row = this.agents.database.prepare('SELECT status FROM agent_runs WHERE org_id = ? AND agent_id = ? AND run_id = ?').get(org, agent, run) as { status: string } | undefined;
    if (!row || row.status !== 'prepared') throw new HttpError(409, 'invalid_run_state', '运行尚未准备或已经开始。');
    if (!this.agents.row(org, agent).enabled) throw new HttpError(409, 'agent_disabled', 'Agent 已停用，不能开始新运行。');
    if (this.agents.get(org, agent).connector.status !== 'available') throw new HttpError(409, 'connector_unavailable', 'Connector 不可用，不能开始新运行。');
    this.agents.database.prepare("UPDATE agent_runs SET status = 'running' WHERE org_id = ? AND agent_id = ? AND run_id = ?").run(org, agent, run);
  }
  finish(org: string, agent: string, run: string, success: boolean) {
    this.agents.row(org, agent);
    const result = this.agents.database.prepare("UPDATE agent_runs SET status = ?, finished_at = ? WHERE org_id = ? AND agent_id = ? AND run_id = ? AND status = 'running'").run(success ? 'complete' : 'error', new Date().toISOString(), org, agent, run);
    if (!result.changes) throw new HttpError(409, 'invalid_run_state', '此运行尚未开始或已经完成。');
  }
}
