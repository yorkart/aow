export type AutomationAgent = 'codex' | 'traecli' | 'claude' | 'hermes' | 'pi';

import type { WorkspaceConfig } from '../workspaces/types';
export type { WorkspaceMode } from '../workspaces/types';

export type TaskKind = 'scheduled' | 'manual';

export interface PromptBinding {
  name: string;
  placeholder: string;
  /** UTF-8 byte offsets into the saved prompt. */
  start: number;
  end: number;
}

export interface TaskInput extends WorkspaceConfig {
  name: string;
  prompt: string;
  kind: TaskKind;
  prompt_bindings: PromptBinding[];
  agent: AutomationAgent;
  project_id: string;
  cleanup_worktree: boolean;
  cron: string;
  interval_seconds: number | null;
  max_concurrent_runs: number;
  enabled: boolean;
  yolo: boolean;
  failure_notification: 'feishu' | 'wechat' | null;
}

export type RunStatus = 'preparing' | 'running' | 'completed' | 'failed' | 'skipped' | 'interrupted';

export type RunOutput = 'stdio' | 'stderr';

export interface AutomationRun {
  id: string;
  task_id: string;
  task_revision: number;
  task_name: string;
  agent: AutomationAgent;
  source: 'scheduled' | 'manual';
  variables?: Record<string, string> | null;
  status: RunStatus;
  started_at: string;
  finished_at: string | null;
  workspace_path: string | null;
  branch: string | null;
  session_id: string | null;
  agent_pid: number | null;
  agent_command: string[] | null;
  exit_code: number | null;
  message: string | null;
  preparation_ms: number | null;
  session_acquired_ms: number | null;
  duration_ms: number | null;
}

export interface AutomationTask extends TaskInput {
  id: string;
  revision: number;
  created_at: string;
  updated_at: string;
  project_name: string;
  scheduler_error: string | null;
  next_run_at: string | null;
  last_run: AutomationRun | null;
  is_running: boolean;
}

export interface SchedulerStatus {
  platform: 'systemd' | 'launchd' | 'unsupported';
  ready: boolean;
  message: string | null;
  timezone: string;
}
