import { appUrl } from '../../lib/basePath';
import type { AgentSessionSnapshot, AowAgentSession } from './types';

export interface SessionShare {
  id: string;
  path: string;
  created_at: string;
}

export class SessionShareError extends Error {
  constructor(message: string, readonly status: number) { super(message); }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(appUrl(path), {
    cache: 'no-store', ...init,
    signal: init?.signal ? AbortSignal.any([init.signal, AbortSignal.timeout(20_000)]) : AbortSignal.timeout(20_000),
  });
  const payload = response.status === 204 ? null : await response.json().catch(() => null);
  if (!response.ok) throw new SessionShareError(payload?.message ?? `请求失败（HTTP ${response.status}）`, response.status);
  return payload as T;
}

const sharePath = (session: AowAgentSession) => `/api/aow/agent-sessions/${encodeURIComponent(session.session_id)}/share`;

export const sessionShareApi = {
  info: (session: AowAgentSession, signal?: AbortSignal) => request<SessionShare | null>(
    `${sharePath(session)}?${new URLSearchParams({ agent: session.agent })}`, { signal }),
  create: (session: AowAgentSession, workspacePath: string) => request<SessionShare>(sharePath(session), {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ agent: session.agent, worktree_path: workspacePath }),
  }),
  revoke: (id: string) => request<void>(`/api/aow/session-shares/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  read: (token: string, signal?: AbortSignal) => request<AgentSessionSnapshot>(
    `/api/public/session-shares/${encodeURIComponent(token)}`, { signal, credentials: 'omit', referrerPolicy: 'no-referrer' }),
};
