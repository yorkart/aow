export interface Project { id: string; org_id: string; name: string; description: string; codebase_path: string | null; directory: string | null; revision: number; task_count: number; created_at: string; updated_at: string }
export interface ProjectFields { name: string; description: string; codebase_path: string }
export type ProjectTab = 'tasks' | 'workspaces' | 'configuration';
export type TaskStatus = 'todo' | 'in_progress' | 'blocked' | 'done' | 'cancelled';
export const taskStatusNames: Record<TaskStatus, string> = { todo: '待办', in_progress: '进行中', blocked: '已阻塞', done: '已完成', cancelled: '已取消' };
export interface TaskLink { id: string; identifier: string; title: string; status: TaskStatus }
export interface Workspace { id: string; org_id: string; project_id: string; name: string; kind: 'project' | 'execution'; directory: string; task_branch: string | null; base_commit: string | null; last_used_at: string | null; created_at: string; is_primary: boolean; available: boolean; availability_message: string; branch: string | null; head: string | null; locked: boolean; prunable: boolean; tasks: TaskLink[] }
export interface WorkspaceList { project: Project; workspaces: Workspace[]; warnings: string[] }
export interface Task extends TaskLink { org_id: string; project_id: string; description: string; agent_id: string | null; workspace_id: string | null; revision: number; created_at: string; updated_at: string; project: Project; agent: { id: string; name: string; enabled: number } | null; workspace: { id: string; directory: string; name: string } | null }
export interface TaskFields { project_id: string; title: string; description: string; status: TaskStatus; agent_id: string | null; workspace_id: string | null }
export interface CodebaseInfo { directory: string; is_git: boolean; root: string | null; branch: string | null; head: string | null; warning: string | null }
export interface DirectoryList { directory: string; parent: string | null; directories: { name: string; path: string }[]; truncated: boolean }
