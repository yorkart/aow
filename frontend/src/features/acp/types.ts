export type Data = Record<string, unknown>;
export const object = (value: unknown): Data => value && typeof value === 'object' && !Array.isArray(value) ? value as Data : {};
export const list = (value: unknown): Data[] => Array.isArray(value) ? value.map(object) : [];
export const text = (value: unknown): string => typeof value === 'string' ? value : '';
export const failure = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
export interface AgentInfo { id: string; name: string; description: string; configured: boolean; supported: boolean; installed: boolean; version?: string; icon?: string | null; repository?: string | null; website?: string | null; license_url?: string | null }
export interface ConnectionInfo { id: string; agent_id: string; cwd: string; agent_info: Data; capabilities: Data; auth_methods: Data[]; prompt_capabilities?: Data; extensions?: Data }
export interface ConnectionStatus { operation_id?: string; phase: string; detail?: string; running: boolean; elapsed_seconds: number }
export interface Permission { id: string; kind: string; request: Data }
export interface ThreadEntry { id: string; kind: string; message_id?: string; content: Data; delivery?: { status: 'sending' | 'injected' | 'started' | 'failed' | 'cancelled' | 'unknown'; error?: string | null } }
export interface SessionTurn { user_entry_id: string; status: 'in_progress' | 'completed' | 'failed' | 'interrupted' | 'unknown'; stop_reason?: string | null; error?: string | null }
export interface TurnOverview {
  id: string; revision: number; user: ThreadEntry[]; conclusion: ThreadEntry[];
  messages?: ThreadEntry[] | null;
  tool_count: number; progress_count: number; running: boolean; waiting: boolean; compacting: boolean;
  status?: SessionTurn['status'] | null; error?: string | null; permission_tool_ids: string[];
}
export interface SessionSnapshot {
  source: 'user' | 'cli'; created_at: string;
  parent?: import('../instances/types').InstanceRef | null;
  thread_id: string; session_id: string | null; agent_id: string; cwd: string; title: string; status: string; revision: number;
  updated_at: string; entries: ThreadEntry[]; permissions: Permission[]; modes: Data; config_options: Data[];
  interacted_at?: string | null;
  turns?: SessionTurn[];
  overview?: TurnOverview[];
  sync_generation?: string;
  commands: Data[]; usage: Data | null; error?: string; stop_reason?: string;
  steering_pending?: boolean; steering_untracked?: boolean;
  needs_load?: boolean;
  recovery?: { attempt_id: string; phase: 'loading' | 'cleaning' | 'failed'; error?: string | null } | null;
  config_options_supported?: boolean; auth_required?: boolean;
  terminals?: Record<string, Data>;
  plan?: Data; notices?: ThreadEntry[];
}
export interface SettingsFile { path: string; content: string; revision: string }
export interface DebugMessage { request_method?: string; timestamp: string; direction: string; message: string; connection_id: string; thread_id: string | null; session_id: string | null; scope: 'thread' | 'connection' | 'unattributed' }

export type SessionInfo = Pick<SessionSnapshot, 'thread_id' | 'parent' | 'source' | 'created_at' | 'session_id' | 'agent_id' | 'cwd' | 'title' | 'status' | 'updated_at' | 'interacted_at'>;

export interface SessionImport { session_id: string; cwd: string; title?: string; updated_at?: string }
