import { t } from '../../lib/i18n';
import type { TaskWorkspaceState } from './workspaceModel';

export type Section = 'tasks' | 'projects' | 'agents' | 'automation';
export type TaskStatus = 'running' | 'review' | 'done' | 'paused';
export type Org = { id: string; name: string; initials: string; taskRoot: string };
export type Project = { id: string; orgId: string; name: string; description: string; repository: string; color: string };
export type Agent = { id: string; orgId: string; name: string; role: string; connector: string; instructions: string; color: string };
export type Task = {
  id: string; orgId: string; title: string; prompt: string; projectId: string; agentId: string;
  origin?: 'manual' | 'automation';
  worktree: boolean; cwd: string; status: TaskStatus; progress: number; time: string; messages: string[];
  workspace?: TaskWorkspaceState;
};
export type Automation = { id: string; orgId: string; name: string; schedule: string; projectId: string; agentId: string; enabled: boolean };
export type Connector = { id: string; name: string; description: string; version: string; installed: boolean };

export const statusLabels: Record<TaskStatus, string> = { running: t('org_preview.running'), review: t('org_preview.awaiting_review'), done: t('org_preview.completed'), paused: t('org_preview.paused') };
export const sectionLabels: Record<Section, string> = { tasks: t('org_preview.tasks'), projects: t('org_preview.projects'), agents: t('org_preview.agents'), automation: t('org_preview.automation') };

export const demoOrgs: Org[] = [
  { id: 'york', name: 'York Studio', initials: 'Y', taskRoot: '/workspace/tasks' },
  { id: 'lab', name: 'Personal Lab', initials: 'P', taskRoot: '/workspace/lab/tasks' },
];
export const demoProjects: Project[] = [
  { id: 'aow', orgId: 'york', name: 'AoW', description: t('org_preview.demo_aow_description'), repository: '/workspace/yorkart/aow', color: 'mint' },
  { id: 'paperclip', orgId: 'york', name: 'Paperclip', description: t('org_preview.demo_paperclip_description'), repository: '/workspace/paperclipai/paperclip', color: 'peach' },
  { id: 'research', orgId: 'york', name: t('org_preview.demo_research_project'), description: t('org_preview.demo_research_description'), repository: '', color: 'violet' },
];
export const demoAgents: Agent[] = [
  { id: 'engineer', orgId: 'york', name: 'Alex', role: t('org_preview.software_engineer'), connector: 'codex', instructions: t('org_preview.demo_engineer_instructions'), color: 'mint' },
  { id: 'designer', orgId: 'york', name: 'Morgan', role: t('org_preview.product_designer'), connector: 'claude', instructions: t('org_preview.demo_designer_instructions'), color: 'peach' },
  { id: 'researcher', orgId: 'york', name: 'Sam', role: t('org_preview.researcher'), connector: 'claude', instructions: t('org_preview.demo_researcher_instructions'), color: 'violet' },
];
export const demoConnectors: Connector[] = [
  { id: 'codex', name: 'Codex', description: t('org_preview.codex_description'), version: 'ACP · stdio', installed: true },
  { id: 'claude', name: 'Claude Code', description: t('org_preview.claude_description'), version: 'ACP · stdio', installed: true },
  { id: 'gemini', name: 'Gemini CLI', description: t('org_preview.gemini_description'), version: 'ACP · stdio', installed: false },
];
export const demoTasks: Task[] = [
  { id: 'AOW-128', orgId: 'york', title: t('org_preview.demo_workspace_title'), prompt: t('org_preview.demo_workspace_prompt'), projectId: 'aow', agentId: 'engineer', worktree: true, cwd: '/workspace/yorkart/aow-AOW-128', status: 'running', progress: 1, time: t('org_preview.just_now'), messages: [] },
  { id: 'AOW-127', orgId: 'york', title: t('org_preview.demo_task_flow_title'), prompt: t('org_preview.demo_task_flow_prompt'), projectId: 'aow', agentId: 'designer', worktree: true, cwd: '/workspace/yorkart/aow-AOW-127', status: 'review', progress: 3, time: t('org_preview.minutes_ago', { count: 18 }), messages: [] },
  { id: 'AOW-126', orgId: 'york', title: t('org_preview.demo_paperclip_title'), prompt: t('org_preview.demo_paperclip_prompt'), projectId: 'paperclip', agentId: 'researcher', worktree: false, cwd: '/workspace/paperclipai/paperclip', status: 'done', progress: 3, time: t('org_preview.hours_ago', { count: 1 }), messages: [] },
  { id: 'AOW-125', orgId: 'york', title: t('org_preview.demo_notes_title'), prompt: t('org_preview.demo_notes_prompt'), projectId: 'research', agentId: 'researcher', worktree: false, cwd: '/workspace/tasks/AOW-125', status: 'done', progress: 3, time: t('org_preview.hours_ago', { count: 2 }), messages: [] },
  { id: 'AOW-124', orgId: 'york', title: t('org_preview.demo_talk_title'), prompt: t('org_preview.demo_talk_prompt'), projectId: '', agentId: 'researcher', worktree: false, cwd: '/workspace/tasks/AOW-124', status: 'done', progress: 3, time: t('org_preview.yesterday'), messages: [] },
  { id: 'AOW-123', orgId: 'york', title: t('org_preview.demo_terminal_title'), prompt: t('org_preview.demo_terminal_prompt'), projectId: 'aow', agentId: 'engineer', worktree: true, cwd: '/workspace/yorkart/aow-AOW-123', status: 'paused', progress: 1, time: t('org_preview.yesterday'), messages: [] },
];
export const demoAutomations: Automation[] = [
  { id: 'daily-review', orgId: 'york', name: t('org_preview.demo_daily_review'), schedule: t('org_preview.demo_daily_schedule'), projectId: 'aow', agentId: 'engineer', enabled: true },
  { id: 'weekly-notes', orgId: 'york', name: t('org_preview.demo_weekly_notes'), schedule: t('org_preview.demo_weekly_schedule'), projectId: 'research', agentId: 'researcher', enabled: false },
];

export function taskDirectory(org: Org, id: string, project: Project | undefined, worktree: boolean) {
  if (!project?.repository) return `${org.taskRoot.replace(/\/+$/, '')}/${id}`;
  return worktree ? `${project.repository.replace(/\/+$/, '')}-${id}` : project.repository;
}
