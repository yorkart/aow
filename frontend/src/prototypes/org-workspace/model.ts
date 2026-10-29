import type { TaskWorkspaceState } from './workspaceModel';

export type Section = 'tasks' | 'projects' | 'agents' | 'automation';
export type TaskStatus = 'running' | 'review' | 'done' | 'paused';
export type Org = { id: string; name: string; initials: string; taskRoot: string };
export type Project = { id: string; orgId: string; name: string; description: string; repository: string; color: string };
export type Agent = { id: string; orgId: string; name: string; role: string; connector: string; instructions: string; color: string };
export type Task = {
  id: string; orgId: string; title: string; prompt: string; projectId: string; agentId: string;
  worktree: boolean; cwd: string; status: TaskStatus; progress: number; time: string; messages: string[];
  workspace?: TaskWorkspaceState;
};
export type Automation = { id: string; orgId: string; name: string; schedule: string; projectId: string; agentId: string; enabled: boolean };
export type Connector = { id: string; name: string; description: string; version: string; installed: boolean };

export const statusLabels: Record<TaskStatus, string> = { running: '执行中', review: '待检查', done: '已完成', paused: '已暂停' };
export const sectionLabels: Record<Section, string> = { tasks: 'Tasks', projects: 'Projects', agents: 'Agents', automation: 'Automation' };

export const demoOrgs: Org[] = [
  { id: 'york', name: 'York Studio', initials: 'Y', taskRoot: '/workspace/tasks' },
  { id: 'lab', name: 'Personal Lab', initials: 'P', taskRoot: '/workspace/lab/tasks' },
];
export const demoProjects: Project[] = [
  { id: 'aow', orgId: 'york', name: 'AoW', description: '让 agent 和人一起把工作完成。', repository: '/workspace/yorkart/aow', color: 'mint' },
  { id: 'paperclip', orgId: 'york', name: 'Paperclip', description: '探索面向 agent 的组织与协作方式。', repository: '/workspace/paperclipai/paperclip', color: 'peach' },
  { id: 'research', orgId: 'york', name: '产品研究', description: '收集想法，研究产品，形成下一步方向。', repository: '', color: 'violet' },
];
export const demoAgents: Agent[] = [
  { id: 'engineer', orgId: 'york', name: 'Alex', role: '软件工程师', connector: 'codex', instructions: '先理解项目约定，再实现改动。完成后运行相关验证，并说明结果。', color: 'mint' },
  { id: 'designer', orgId: 'york', name: 'Morgan', role: '产品设计师', connector: 'claude', instructions: '从用户任务出发设计清晰的流程，提供可交互的界面方案。', color: 'peach' },
  { id: 'researcher', orgId: 'york', name: 'Sam', role: '研究员', connector: 'claude', instructions: '整理证据、比较方案，并给出可以执行的建议。', color: 'violet' },
];
export const demoConnectors: Connector[] = [
  { id: 'codex', name: 'Codex', description: 'OpenAI 的编码 agent，通过 ACP 连接。', version: 'ACP · stdio', installed: true },
  { id: 'claude', name: 'Claude Code', description: 'Anthropic 的编码 agent，通过 ACP 连接。', version: 'ACP · stdio', installed: true },
  { id: 'gemini', name: 'Gemini CLI', description: 'Google 的命令行 agent，通过 ACP 连接。', version: 'ACP · stdio', installed: false },
];
export const demoTasks: Task[] = [
  { id: 'AOW-128', orgId: 'york', title: '让工作围绕 task 展开', prompt: '为 AoW 设计以 task 为入口的工作空间。增加组织和 agent 的概念，保留熟悉的三栏布局，让项目与执行环境跟随任务。', projectId: 'aow', agentId: 'engineer', worktree: true, cwd: '/workspace/yorkart/aow-AOW-128', status: 'running', progress: 1, time: '刚刚', messages: [] },
  { id: 'AOW-127', orgId: 'york', title: '优化新建任务的交互流程', prompt: '梳理任务创建时的 agent、project 和工作目录选择，减少不必要的步骤。', projectId: 'aow', agentId: 'designer', worktree: true, cwd: '/workspace/yorkart/aow-AOW-127', status: 'review', progress: 3, time: '18 分钟前', messages: [] },
  { id: 'AOW-126', orgId: 'york', title: '梳理 Paperclip 的组织模型', prompt: '研究 Paperclip 的 company、project、agent 和 issue 之间的关系。', projectId: 'paperclip', agentId: 'researcher', worktree: false, cwd: '/workspace/paperclipai/paperclip', status: 'done', progress: 3, time: '1 小时前', messages: [] },
  { id: 'AOW-125', orgId: 'york', title: '整理本周的产品想法', prompt: '把本周收集的产品想法整理成一份简洁的研究笔记。', projectId: 'research', agentId: 'researcher', worktree: false, cwd: '/workspace/tasks/AOW-125', status: 'done', progress: 3, time: '2 小时前', messages: [] },
  { id: 'AOW-124', orgId: 'york', title: '帮我拟一份技术分享提纲', prompt: '准备一份关于 agent 协作的技术分享提纲，面向前端工程师。', projectId: '', agentId: 'researcher', worktree: false, cwd: '/workspace/tasks/AOW-124', status: 'done', progress: 3, time: '昨天', messages: [] },
  { id: 'AOW-123', orgId: 'york', title: '修复终端恢复时的尺寸问题', prompt: '检查终端恢复时的布局计算，避免窗口尺寸变化导致内容截断。', projectId: 'aow', agentId: 'engineer', worktree: true, cwd: '/workspace/yorkart/aow-AOW-123', status: 'paused', progress: 1, time: '昨天', messages: [] },
];
export const demoAutomations: Automation[] = [
  { id: 'daily-review', orgId: 'york', name: '检查 AoW 的待审查改动', schedule: '每个工作日 · 09:00', projectId: 'aow', agentId: 'engineer', enabled: true },
  { id: 'weekly-notes', orgId: 'york', name: '整理每周产品研究', schedule: '每周五 · 17:00', projectId: 'research', agentId: 'researcher', enabled: false },
];

export function taskDirectory(org: Org, id: string, project: Project | undefined, worktree: boolean) {
  if (!project?.repository) return `${org.taskRoot.replace(/\/+$/, '')}/${id}`;
  return worktree ? `${project.repository.replace(/\/+$/, '')}-${id}` : project.repository;
}
