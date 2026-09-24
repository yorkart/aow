import type { AowAgentSession } from '../sessions/types';
export type TerminalSplitAxis = 'row' | 'column';

export type TerminalPaneStatus = 'running' | 'exited' | 'interrupted';

export interface TerminalPane {
  id: string;
  name: string;
  cwd: string;
  shell: string;
  arguments?: string[];
  kind?: 'terminal' | 'agent';
  agent_id?: string | null;
  agent_terminal?: {
    phase: 'starting' | 'ready' | 'failed';
    error: string | null;
    task_submitted: boolean;
  };
  restart_on_daemon_restart?: boolean;
  status: TerminalPaneStatus;
  exit_code?: number | null;
  rows?: number;
  cols?: number;
}

export interface TerminalPaneLayout {
  type: 'pane';
  pane_id: string;
}

export interface TerminalSplitLayout {
  type: 'split';
  axis: TerminalSplitAxis;
  ratio: number;
  first: TerminalLayout;
  second: TerminalLayout;
}

export type TerminalLayout = TerminalPaneLayout | TerminalSplitLayout;

export interface TerminalTab {
  id: string;
  name: string;
  name_is_custom?: boolean;
  workspace_root: string;
  layout: TerminalLayout | null;
  panes: TerminalPane[];
  revision?: number | string;
  created_at?: string | number;
  updated_at?: string | number;
}

export interface TerminalAgentList {
  agents: Record<string, string | null>;
  titles: Record<string, string>;
  processes: Record<string, TerminalAgentProcess>;
}

export interface TerminalAgentProcess {
  pid: number;
  start_time: string;
  cwd: string;
}

export interface TerminalPaneSessions {
  agent: AowAgentSession['agent'];
  cwd: string;
  title: string;
  process: TerminalAgentProcess | null;
  live_session_id: string | null;
  sessions: AowAgentSession[];
}
