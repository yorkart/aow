import type { MobileRoute } from '../../mobile/mobileState';
import type { TabTarget, ResolvedTab } from './types';
import type { AowAgentSession } from '../../features/sessions/types';
import type { AowProject, AowWorktree } from '../types';

type MobileContext = { route: Partial<MobileRoute>; project: AowProject; worktree: AowWorktree };
interface MobileAdapter {
  restore: (target: TabTarget) => Partial<MobileRoute>;
  capture: (context: MobileContext) => TabTarget | undefined;
}
// These adapters translate the same resource routes into the existing mobile readers.
const adapters: Record<TabTarget['type'], MobileAdapter> = {
  terminal: {
    restore: t => t.type === 'terminal' ? { view: 'terminal', terminal: t.tabId } : {},
    capture: () => undefined, // The terminal pane reports its active Tab after loading.
  },
  files: {
    restore: t => t.type === 'files' ? { view: 'files', directory: t.path, fileSource: 'external' } : {},
    capture: ({ route: r, worktree: w, project: p }) => r.view === 'files' && !r.file ? { type: 'files', workspace: w.id, path: r.directory ?? (r.notes === '1' ? p.notes_path : w.path) } : undefined,
  },
  file: {
    restore: t => t.type === 'file' ? { view: 'files', file: t.path, directory: t.path.slice(0, t.path.lastIndexOf('/')) || '/', fileSource: t.source, notes: t.source === 'notes' ? '1' : undefined } : {},
    capture: ({ route: r, worktree: w }) => r.view === 'files' && r.file ? { type: 'file', workspace: w.id, path: r.file, source: r.fileSource ?? (r.notes === '1' ? 'notes' : 'project') } : undefined,
  },
  diff: {
    restore: t => t.type === 'diff' ? { view: 'git', diff: t.path, repository: t.repository, staged: t.source === 'staged' ? '1' : undefined, commit: t.commit, originalPath: t.originalPath, untracked: t.untracked ? '1' : undefined } : {},
    capture: ({ route: r, worktree: w }) => r.view === 'git' && r.diff ? { type: 'diff', workspace: w.id, path: r.diff, repository: r.repository ?? w.path, source: r.commit ? 'commit' : r.staged === '1' ? 'staged' : 'working', commit: r.commit, originalPath: r.originalPath, untracked: r.untracked === '1' || undefined } : undefined,
  },
  pr: {
    restore: t => t.type === 'pr' ? { view: 'pull-requests', pr: String(t.number), provider: t.provider, remote: t.remote, repository: t.repository } : {},
    capture: ({ route: r, worktree: w }) => r.view === 'pull-requests' && r.pr ? { type: 'pr', workspace: w.id, repository: r.repository ?? w.path, provider: r.provider ?? 'auto', remote: r.remote, number: Number(r.pr) } : undefined,
  },
  session: {
    restore: t => t.type === 'session' ? { view: 'sessions', session: t.agent + ':' + t.sessionId, agent: t.agent, cwd: t.cwd } : {},
    capture: ({ route: r, worktree: w }) => r.view === 'sessions' && r.session && r.agent ? { type: 'session', workspace: w.id, agent: r.agent as AowAgentSession['agent'], sessionId: r.session.startsWith(r.agent + ':') ? r.session.slice(r.agent.length + 1) : r.session, cwd: r.cwd ?? w.path } : undefined,
  },
  automation: {
    restore: t => t.type === 'automation' ? { view: 'automations', task: t.taskId, run: t.runId, automationView: t.view } : {},
    capture: ({ route: r, worktree: w }) => r.view === 'automations' && r.task ? { type: 'automation', workspace: w.id, taskId: r.task, runId: r.run, view: r.run || r.automationView === 'runs' ? 'runs' : undefined } : undefined,
  },
};
export function mobileRouteForTab(entry: ResolvedTab): MobileRoute {
  return { workspace: entry.workspacePath, view: 'terminal', ...adapters[entry.target.type].restore(entry.target) };
}
export function mobileTabTarget(route: Partial<MobileRoute>, projects: AowProject[]) {
  const project = projects.find(p => p.worktrees.some(w => w.path === route.workspace));
  const worktree = project?.worktrees.find(w => w.path === route.workspace);
  if (!project || !worktree) return;
  for (const adapter of Object.values(adapters)) {
    const target = adapter.capture({ route, project, worktree });
    if (target) return target;
  }
}
