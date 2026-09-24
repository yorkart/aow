import type { TaskInput, RunOutput, AutomationRun, AutomationTask, SchedulerStatus } from './types';
import { appUrl } from '../../lib/basePath';

const base = appUrl('/api/aow/automations');
async function request<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await fetch(`${base}${path}`, {
    method, headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => null) as { message?: string } | null;
    throw new Error(payload?.message ?? `请求失败 (${response.status})`);
  }
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}
export const automationApi = {
  status: () => request<SchedulerStatus>('/status'),
  list: (projectId?: string) => request<AutomationTask[]>(projectId ? `?project_id=${encodeURIComponent(projectId)}` : ''),
  detail: (id: string) => request<AutomationTask>(`/${id}`),
  create: (input: TaskInput) => request<AutomationTask>('', 'POST', input),
  update: (id: string, input: TaskInput, revision: number) => request<AutomationTask>(`/${id}`, 'PUT', { ...input, revision }),
  enabled: (id: string, enabled: boolean) => request<AutomationTask>(`/${id}/enabled`, 'PUT', { enabled }),
  sync: (id: string) => request<AutomationTask>(`/${id}/sync`, 'POST'),
  remove: (id: string) => request<void>(`/${id}`, 'DELETE'),
  run: (id: string, revision: number, variables: Record<string, string> = {}) => request<{ run_id: string | null }>(`/${id}/run`, 'POST', { revision, variables }),
  runs: (id: string, before?: string, limit = 50) => request<AutomationRun[]>(`/${id}/runs?limit=${limit}${before ? `&before=${encodeURIComponent(before)}` : ''}`),
  runDetail: (id: string, runId: string) => request<AutomationRun>(`/${id}/runs/${runId}`),
  runOutput: async (id: string, runId: string, output: RunOutput) => {
    const response = await fetch(`${base}/${encodeURIComponent(id)}/runs/${encodeURIComponent(runId)}/output/${output}`);
    if (!response.ok) {
      const payload = await response.json().catch(() => null) as { message?: string } | null;
      throw new Error(payload?.message ?? `请求失败 (${response.status})`);
    }
    return response.text();
  },
};
