import type { WorkspaceConfig, WorkspaceMode } from '../workspaces/types';

export interface InboxLabel { id: string; name: string; color: string }
export interface InboxComment {
  id: string; author: { type: 'human' | 'ai'; name: string }; created_at: string; content: string;
}
export interface InboxItem {
  id: string; markdown: string; project_id: string | null; label_ids: string[];
  revision: number; created_at: string; updated_at: string;
}
export interface InboxExecution {
  id: string; item_id: string; request_key: string; item_revision: number;
  agent: string; project_id: string; cwd: string; markdown: string; workspace_mode: WorkspaceMode;
  phase: 'starting' | 'submitted' | 'failed' | 'interrupted';
  tab_id: string | null; pane_id: string | null; error: string | null; created_at: string;
}
export type InboxWorkspaceMode = WorkspaceMode;
export interface InboxExecuteOptions extends WorkspaceConfig {
  agent: string;
  append_prompt: string;
}
export interface InboxSnapshot { revision: number; labels: InboxLabel[]; items: InboxItem[]; executions: InboxExecution[]; comment_counts: Record<string, number> }
export const previewTitle = (markdown: string) => markdown.split(/\r?\n/, 1)[0].replace(/^\s{0,3}#{1,6}\s+/, '').trim() || '未命名需求';
export const inboxError = (error: unknown) => error instanceof Error ? error.message : String(error);
