import { acpRequest } from '../acp/request';
import type { AowAgentType } from '../agents/types';
import type { SessionSnapshot } from '../acp/types';
import type { TerminalTab } from '../terminals/types';
import { taskHeaders } from '../tasks/api';

export type HomeLayout = AowAgentType | 'gemini' | 'opencode' | 'custom';
export type RoleRuntime = { mode: 'acp' | 'cli'; agent_id: string };
export interface RoleConfig {
  name: string;
  description?: string;
  runtime: RoleRuntime;
  home_layout?: HomeLayout;
  identity: { kind: 'local_home'; source: string | null };
  instruction: string;
  skills: string[];
}
export interface AgentRole extends RoleConfig { id: string; home_layout: HomeLayout }
export interface IdentitySource { layout: HomeLayout; source: string }
export type RoleSession = { kind: 'acp'; session: SessionSnapshot } | { kind: 'cli'; terminal: TerminalTab };
export const rolesChanged = 'aow:agent-roles-changed';
const root = '/api/aow/agent-roles';
const rolePath = (id: string) => `${root}/${encodeURIComponent(id)}`;
async function mutate<T>(url: string, method: string, body?: unknown): Promise<T> {
  const result = await acpRequest<T>(url, { method, body: body === undefined ? undefined : JSON.stringify(body) });
  window.dispatchEvent(new Event(rolesChanged));
  return result;
}
export const rolesApi = {
  list: () => acpRequest<AgentRole[]>(root),
  sources: () => acpRequest<IdentitySource[]>(`${root}/identity-sources`),
  save: (config: RoleConfig, id?: string) => mutate<AgentRole>(id ? rolePath(id) : root, id ? 'PUT' : 'POST', config),
  testAndCreate: (config: RoleConfig, request_id: string) => mutate<AgentRole>(`${root}/test-and-create`, 'POST', { ...config, request_id }),
  remove: (id: string) => mutate<void>(rolePath(id), 'DELETE'),
  launch: (id: string, project_id: string, cwd: string, taskId?: string, parent_thread_id?: string) => acpRequest<RoleSession>(`${rolePath(id)}/sessions`, { method: 'POST', headers: taskHeaders(taskId), body: JSON.stringify({ project_id, cwd, parent_thread_id }) }),
};
