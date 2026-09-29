export interface InboxItem { id: string; project_id: string; revision: number; title: string; description: string; created_at: string; updated_at: string }
export interface InboxSummary extends Omit<InboxItem, 'description'> { task_ids: string[] }
export interface InboxPage { items: InboxSummary[]; total: number; next_cursor: string | null }
export interface TaskStatus { id: string; name: string; color: string }
export interface TaskStatusWrite { id?: string; name: string; color: string }
export interface BoardTask {
  id: string; revision: number; inbox_id: string; status_id: string;
  title: string; description: string; project_id: string; cwd: string; agent: string;
  tab_id: string | null; pane_id: string | null;
  execution: 'preparing' | 'ready' | 'submitting' | 'submitted' | 'failed'; error: string | null;
  archived: boolean; created_at: string; updated_at: string;
  history: { status_id: string; at: string; reason: string }[];
}
export interface TaskBoardData { version: number; status_revision: number; statuses: TaskStatus[]; tasks: BoardTask[] }
export interface ConvertInput {
  request_key: string; expected_revision: number; title: string; description: string;
  status_id: string; project_id: string; cwd: string; agent: string; start_now: boolean;
  worktree: { branch: string; base_ref: string } | null;
}
