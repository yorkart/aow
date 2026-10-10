import { acpRequest } from './request';
import type { AgentInfo, ConnectionInfo, ConnectionStatus, Data, DebugMessage, Permission, SessionImport, SessionInfo, SessionSnapshot, SettingsFile } from './types';
const root = '/api/zed';
export const settingsChanged = 'aow:zed-settings-changed';
export const sessionsChanged = 'aow:zed-sessions-changed';
export const authenticationCompleted = 'aow:zed-authentication-completed';
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
  create: (connection: string) => acpRequest<SessionSnapshot>(`${root}/connections/${encodeURIComponent(connection)}/sessions`, post()),
  remoteSessions: (connection: string, cursor?: string) => acpRequest<{ sessions: Data[]; nextCursor?: string }>(`${root}/connections/${encodeURIComponent(connection)}/sessions${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`),
  importSessions: (connection: string, sessions: SessionImport[]) => acpRequest<SessionInfo[]>(`${root}/connections/${encodeURIComponent(connection)}/import`, post(sessions)),
  load: (connection: string, remote_id: string) => acpRequest<SessionSnapshot>(`${root}/connections/${encodeURIComponent(connection)}/load`, post({ remote_id })),
  sessions: (cwd: string, signal?: AbortSignal) => acpRequest<SessionInfo[]>(`${root}/sessions?cwd=${encodeURIComponent(cwd)}`, { signal }),
  snapshot: (id: string, signal?: AbortSignal) => acpRequest<SessionSnapshot>(`${root}/sessions/${encodeURIComponent(id)}`, { signal }),
  action: (id: string, action: Data) => acpRequest<SessionSnapshot>(`${root}/sessions/${encodeURIComponent(id)}/actions`, post(action)),
  remove: (id: string) => acpRequest(`${root}/sessions/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  logs: (id: string) => acpRequest<DebugMessage[]>(`${root}/sessions/${encodeURIComponent(id)}/logs`),
};
