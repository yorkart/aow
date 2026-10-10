export type Data = Record<string, unknown>;
export const object = (value: unknown): Data => value && typeof value === 'object' && !Array.isArray(value) ? value as Data : {};
export const list = (value: unknown): Data[] => Array.isArray(value) ? value.map(object) : [];
export const text = (value: unknown): string => typeof value === 'string' ? value : '';
export const failure = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
export interface AgentInfo { id: string; name: string; description: string; configured: boolean; supported: boolean; installed: boolean; version?: string }
export interface ConnectionInfo { id: string; agent_id: string; cwd: string; agent_info: Data; capabilities: Data; auth_methods: Data[]; prompt_capabilities?: Data }
export interface ConnectionStatus { phase: string; detail?: string; running: boolean; elapsed_seconds: number }
export interface Permission { id: string; kind: string; request: Data }
export interface ThreadEntry { id: string; kind: string; content: Data }
export interface SessionSnapshot {
  id: string; remote_id: string; agent_id: string; cwd: string; title: string; status: string; revision: number;
  updated_at: string; entries: ThreadEntry[]; permissions: Permission[]; modes: Data; config_options: Data[];
  commands: Data[]; usage: Data | null; error?: string; stop_reason?: string;
  needs_load?: boolean;
  config_options_supported?: boolean; auth_required?: boolean;
  terminals?: Record<string, Data>;
  plan?: Data; notices?: ThreadEntry[];
}
export interface SettingsFile { path: string; content: string; revision: string }
export interface DebugMessage { timestamp: string; direction: string; message: string }

export type SessionInfo = Pick<SessionSnapshot, 'id' | 'remote_id' | 'agent_id' | 'cwd' | 'title' | 'status' | 'updated_at'>;

export interface SessionImport { remote_id: string; cwd: string; title?: string; updated_at?: string }
