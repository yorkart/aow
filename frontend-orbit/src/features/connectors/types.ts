export type AuthMethod = 'local' | 'independent' | 'api_key';
export type ConnectorStatus = 'available' | 'auth_required' | 'error' | 'disabled';
export interface Connector {
  id: string; org_id: string; name: string; provider: string; executor: string; auth_method: AuthMethod;
  account_note: string; account_summary: string; status: ConnectorStatus; status_message: string;
  enabled: boolean; revision: number; credential_configured: boolean; checked_at: string | null; updated_at: string;
  models: string[]; agents: { agent_id: string }[]; agent_count: number;
}
export interface Provider { id: string; name: string; executors: string[]; auth_methods: AuthMethod[] }
export interface Catalog { providers: Provider[]; executors: { id: string; name: string; installed: boolean }[] }
export interface ConnectorFields { name: string; provider: string; executor: string; auth_method: AuthMethod; account_note: string }
export interface CheckResult { status: 'available' | 'auth_required' | 'error'; message: string; summary?: string; models?: string[] }
export interface AuthSession {
  id: string; provider: string; executor: string; status: 'starting' | 'waiting' | 'completing' | 'complete' | 'error' | 'cancelled' | 'expired';
  verification_uri: string | null; user_code: string | null; message: string; account_summary: string; expires_at: number;
}
export const providerNames: Record<string, string> = { openai: 'OpenAI', anthropic: 'Anthropic', deepseek: 'DeepSeek', gemini: 'Google Gemini' };
export const executorNames: Record<string, string> = { codex: 'Codex', claude: 'Claude Code', gemini: 'Gemini CLI' };
export const authNames: Record<AuthMethod, string> = { local: '本地登录', independent: '独立登录', api_key: 'API Key' };
export const statusNames: Record<ConnectorStatus, string> = { available: '可用', auth_required: '待认证', error: '异常', disabled: '已停用' };
