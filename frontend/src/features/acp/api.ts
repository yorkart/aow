import { acpRequest } from './request';
import { acpControlHeaders } from '../acp-control/sessionControl';
import type { AgentInfo, ConnectionInfo, ConnectionStatus, Data, DebugMessage, Permission, SessionImport, SessionInfo, SessionSnapshot, SettingsFile } from './types';
import type { SessionUpdates } from './session_updates';
const root = '/api/acp';
export const settingsChanged = 'aow:acp-settings-changed';
export const sessionsChanged = 'aow:acp-sessions-changed';
export const authenticationCompleted = 'aow:acp-authentication-completed';
const post = (body?: unknown): RequestInit => ({ method: 'POST', body: JSON.stringify(body ?? {}) });
export const acpApi = {
  agents: () => acpRequest<AgentInfo[]>(`${root}/agents`),
  registry: (refresh = false) => acpRequest<AgentInfo[]>(`${root}/registry?refresh=${refresh}`),
  install: async (id: string) => {
    await acpRequest(`${root}/agents/${encodeURIComponent(id)}/installation`, post());
    window.dispatchEvent(new Event(settingsChanged));
  },
  installationStatus: (id: string, signal: AbortSignal) => acpRequest<ConnectionStatus | null>(`${root}/agents/${encodeURIComponent(id)}/installation`, { signal }),
  settings: () => acpRequest<SettingsFile>(`${root}/settings`),
  saveSettings: async (content: string, revision: string) => {
    const saved = await acpRequest<SettingsFile>(`${root}/settings`, { method: 'PUT', body: JSON.stringify({ content, revision }) });
    window.dispatchEvent(new Event(settingsChanged));
    return saved;
  },
  connect: (agent_id: string, cwd: string) => acpRequest<ConnectionInfo>(`${root}/connections`, post({ agent_id, cwd })),
  connectionStatus: (agent_id: string, cwd: string, signal: AbortSignal) => acpRequest<ConnectionStatus | null>(`${root}/connection-status?${new URLSearchParams({ agent_id, cwd })}`, { signal }),
  requests: (id: string, signal?: AbortSignal) => acpRequest<Permission[]>(`${root}/connections/${encodeURIComponent(id)}/requests`, { signal }),
  answerConnection: (id: string, request_id: string, response: Data) => acpRequest(`${root}/connections/${encodeURIComponent(id)}/requests`, post({ request_id, response })),
  authenticate: async (id: string, method_id: string) => {
    await acpRequest(`${root}/connections/${encodeURIComponent(id)}/authenticate`, post({ method_id }));
    window.dispatchEvent(new CustomEvent(authenticationCompleted, { detail: id }));
  },
  create: (agent_id: string, cwd: string) => acpRequest<SessionSnapshot>(`${root}/sessions`, post({ agent_id, cwd })),
  remoteSessions: (connection: string, cursor?: string) => acpRequest<{ sessions: Data[]; nextCursor?: string }>(`${root}/connections/${encodeURIComponent(connection)}/sessions${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`),
  importSessions: (connection: string, sessions: SessionImport[]) => acpRequest<SessionInfo[]>(`${root}/connections/${encodeURIComponent(connection)}/import`, post(sessions)),
  sessions: (cwd: string, signal?: AbortSignal) => acpRequest<SessionInfo[]>(`${root}/sessions?cwd=${encodeURIComponent(cwd)}`, { signal }),
  info: (thread_id: string, signal?: AbortSignal) => acpRequest<SessionInfo>(`${root}/sessions/${encodeURIComponent(thread_id)}/metadata`, { signal }),
  connection: (thread_id: string, signal?: AbortSignal) => acpRequest<ConnectionInfo | null>(`${root}/sessions/${encodeURIComponent(thread_id)}/connection`, { signal }),
  snapshot: (thread_id: string, signal?: AbortSignal) => acpRequest<SessionSnapshot>(`${root}/sessions/${encodeURIComponent(thread_id)}?view=overview`, { signal }),
  updates: async (thread_id: string, current: SessionSnapshot | undefined, signal?: AbortSignal): Promise<SessionUpdates> => {
    const path = `${root}/sessions/${encodeURIComponent(thread_id)}`;
    if (!current?.sync_generation || !current.overview) return { kind: 'snapshot', snapshot: await acpRequest<SessionSnapshot>(`${path}?view=overview`, { signal }) };
    // Preparation can return a canonical snapshot; it must not select the
    // browser's synchronization mode while resume and polling race.
    const query = new URLSearchParams({ generation: current.sync_generation, revision: String(current.revision), view: 'overview' });
    return acpRequest<SessionUpdates>(`${path}/updates?${query}`, { signal });
  },
  process: (thread_id: string, turn_id: string, current?: SessionSnapshot, signal?: AbortSignal) => {
    const query = new URLSearchParams({ turn_id });
    if (current?.sync_generation) { query.set('generation', current.sync_generation); query.set('revision', String(current.revision)); }
    return acpRequest<SessionUpdates>(`${root}/sessions/${encodeURIComponent(thread_id)}/process?${query}`, { signal });
  },
  action: (thread_id: string, action: Data, token: string) => acpRequest<SessionSnapshot>(`${root}/sessions/${encodeURIComponent(thread_id)}/actions?view=overview`, { ...post(action), headers: { 'X-Aow-Acp-Control': token } }),
  remove: (thread_id: string) => acpRequest(`${root}/sessions/${encodeURIComponent(thread_id)}`, { method: 'DELETE', headers: acpControlHeaders(thread_id) }),
  logs: (thread_id: string) => acpRequest<DebugMessage[]>(`${root}/sessions/${encodeURIComponent(thread_id)}/logs`),
};
