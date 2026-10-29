export type ProviderId = 'openai' | 'anthropic' | 'deepseek' | 'gemini';
export type ExecutorId = 'codex' | 'claude' | 'gemini';
export type AuthMethod = 'local' | 'independent' | 'api_key';
export type ConnectorStatus = 'available' | 'auth_required' | 'error' | 'disabled';
export interface ConnectorRow {
  id: string; org_id: string; name: string; provider: ProviderId; executor: ExecutorId; auth_method: AuthMethod;
  account_note: string; account_summary: string; source_root: string | null; credentials: string | null;
  status: ConnectorStatus; status_message: string; enabled: number; revision: number; models_json: string;
  checked_at: string | null; updated_at: string;
}
export interface CredentialFile { path: string; content: string }
export interface Credentials { kind: 'files' | 'api_key' | 'claude_token'; files?: CredentialFile[]; key?: string; token?: string }
export interface Identity { credentials: Credentials; summary: string; sourceRoot?: string }
export interface CheckResult { status: 'available' | 'auth_required' | 'error'; message: string; summary?: string; models?: string[] }
export interface ExecutorInfo { id: ExecutorId; name: string; installed: boolean }
export interface ProviderInfo { id: ProviderId; name: string; executors: ExecutorId[]; auth_methods: AuthMethod[] }
export interface AuthSession {
  id: string; org_id: string; provider: ProviderId; executor: ExecutorId;
  status: 'starting' | 'waiting' | 'completing' | 'complete' | 'error' | 'cancelled' | 'expired';
  verification_uri: string | null; user_code: string | null; message: string; credentials: string | null;
  account_summary: string; expires_at: number; created_at: string;
}
