export interface AowAgentSession {
  id: string;
  agent: 'claude' | 'codex' | 'traecli' | 'hermes';
  session_id: string;
  title: string;
  cwd: string;
  created_at: string;
  updated_at: string;
}

export type AgentSessionTurnStatus = 'completed' | 'failed' | 'interrupted' | 'in_progress';

export interface AgentSessionSnapshotMessage {
  text: string;
  timestamp: string | null;
}

export type AgentSessionToolAction = 'read_files' | 'search_files' | 'list_files' | 'edit_files' | 'run_commands'
  | 'load_tools' | 'search_web' | 'update_plan' | 'wait' | 'orchestrate' | 'other';

export interface AgentSessionActivity {
  id: string;
  kind: 'commentary' | 'tool';
  text: string;
  timestamp: string | null;
  status?: AgentSessionTurnStatus | 'unknown';
  actions?: AgentSessionToolAction[];
  details?: AgentSessionToolDetails;
}

export interface AgentSessionToolText {
  text: string;
  truncated: boolean;
}

export interface AgentSessionToolDetails {
  command?: AgentSessionToolText;
  input?: AgentSessionToolText;
  cwd?: AgentSessionToolText;
  output?: AgentSessionToolText;
  error?: AgentSessionToolText;
  exit_code?: number;
  duration_ms?: number;
}

export interface AgentSessionSnapshotTurn {
  id: string;
  status: AgentSessionTurnStatus;
  user: AgentSessionSnapshotMessage;
  final: AgentSessionSnapshotMessage | null;
  activities?: AgentSessionActivity[];
  activities_truncated?: boolean;
}

export interface AgentSessionSnapshot {
  session_id: string;
  agent: AowAgentSession['agent'];
  title: string;
  captured_at: string;
  status: AgentSessionTurnStatus;
  turns: AgentSessionSnapshotTurn[];
  truncated: boolean;
}
