export interface AgentRuntime { model: string | null; reasoning_effort: string | null; timeout_seconds: number }
export interface AgentRow {
  id: string; org_id: string; name: string; role: string; description: string; enabled: number;
  runtime_json: string; instructions_version_id: string; instructions_updated_at: string;
  identity_revision: number; runtime_revision: number; skills_revision: number; variables_revision: number;
  created_at: string; updated_at: string;
}
export interface VariableRow { id: string; org_id: string; agent_id: string; name: string; kind: 'variable' | 'secret'; value: string | null; secret_id: string | null }
export interface ModelOption { id: string; reasoning_efforts: string[] }
export const defaultRuntime: AgentRuntime = { model: null, reasoning_effort: null, timeout_seconds: 900 };
