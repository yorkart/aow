import { aowRequest } from '../../lib/aowRequest';
import { acpControlHeaders } from '../acp-control/sessionControl';

export interface WorkTask {
  id: string; project_id: string; parent_id: string | null; prompt: string;
  workspace_mode?: 'existing' | 'new_worktree'; slug?: string | null;
  agent_role_id?: string | null; session_id?: string | null;
  agent_runtime_id: string; workspace: string; thread_id: string; pinned: boolean; archived: boolean;
}
export interface CreateTask {
  request_id: string; project_id: string; parent_id?: string; prompt: string; agent_runtime_id: string; workspace?: string; workspace_mode?: 'existing' | 'new_worktree'; slug?: string;
}
const root = '/api/aow/tasks';
export const tasksApi = {
  list: () => aowRequest<WorkTask[]>(root, { cache: 'no-store' }),
  create: (input: CreateTask) => aowRequest<WorkTask>(root, { method: 'POST', body: JSON.stringify(input) }),
  pin: (id: string, pinned: boolean) => aowRequest<void>(`${root}/${encodeURIComponent(id)}`, { method: 'PATCH', body: JSON.stringify({ pinned }) }),
  finish: (task: WorkTask, tasks: WorkTask[], archive: boolean) => {
    const controls = Object.fromEntries(tasks.map(item => [item.thread_id, acpControlHeaders(item.thread_id)['X-Aow-Acp-Control'] ?? acpControlHeaders(item.thread_id)['x-aow-acp-control']]).filter((entry): entry is [string, string] => typeof entry[1] === 'string'));
    return aowRequest<void>(`${root}/${encodeURIComponent(task.id)}${archive ? '/archive' : ''}`, { method: archive ? 'POST' : 'DELETE', body: JSON.stringify({ controls }) });
  },
};
