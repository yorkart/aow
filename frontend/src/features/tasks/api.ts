import { acpRequest } from '../acp/request';
import { mentionPreview } from '../agent-roles/mentions';

export interface TaskResource { kind: 'acp' | 'terminal'; id: string }
export interface WorkTask {
  id: string;
  project_id: string;
  prompt: string;
  agent_id: string;
  agent_role_id?: string | null;
  is_router?: boolean;
  source_task_id?: string | null;
  source_thread_id?: string | null;
  workspace_intent?: 'change' | 'analyze' | 'review' | null;
  target_task_id?: string | null;
  target_revision?: string | null;
  workspace: string;
  workspace_mode: 'existing' | 'new_worktree';
  slug: string | null;
  initial_thread_id: string | null;
  pinned: boolean;
  archived: boolean;
  dependencies: string[];
  resources: TaskResource[];
}
export const taskScopeKey = (id: string) => `task:${id}`;
export const taskTitle = (task: WorkTask) => task.is_router ? 'Router' : mentionPreview(task.prompt.split('\n')[0]);
export interface TaskResult {
  task: WorkTask;
  runtime: { thread_id: string; status: string; run_state?: string | null; error?: string | null; response?: string; pending_permissions?: number } | null;
  delegated: WorkTask[];
}
export const tasksChanged = 'aow:tasks-changed';
export const notifyTasksChanged = () => window.dispatchEvent(new Event(tasksChanged));
export const taskHeaders = (taskId?: string) => taskId ? { 'X-Aow-Task': taskId } : undefined;
const root = '/api/aow/tasks';
const path = (id: string) => `${root}/${encodeURIComponent(id)}`;
async function mutate<T>(url: string, method: string, body?: unknown): Promise<T> {
  const result = await acpRequest<T>(url, { method, body: body === undefined ? undefined : JSON.stringify(body) });
  notifyTasksChanged();
  return result;
}
export const tasksApi = {
  router: (projectId: string, input: { agent_id?: string; agent_role_id?: string }) => mutate<WorkTask>(`/api/aow/projects/${encodeURIComponent(projectId)}/router`, 'POST', input),
  get: (id: string) => acpRequest<TaskResult>(path(id), { cache: 'no-store' }),
  list: (signal?: AbortSignal) => acpRequest<WorkTask[]>(root, { signal, cache: 'no-store' }),
  create: (input: Pick<WorkTask, 'project_id' | 'prompt' | 'agent_id' | 'agent_role_id' | 'dependencies'> & { request_id: string; slug?: string } & ({ workspace_mode: 'existing'; workspace: string } | { workspace_mode: 'new_worktree'; workspace?: never })) => mutate<WorkTask>(root, 'POST', input),
  update: (id: string, input: Partial<Pick<WorkTask, 'prompt' | 'pinned' | 'archived' | 'dependencies'>>) => mutate<void>(path(id), 'PATCH', input),
  remove: (id: string) => mutate<void>(path(id), 'DELETE'),
  attach: (id: string, resource: TaskResource) => mutate<void>(`${path(id)}/resources`, 'POST', resource),
};
