import { appUrl } from '../../lib/basePath';
import { terminalApi } from '../../features/terminals/terminalApi';
import { aowApi } from '../aowApi';
import type { ResolvedTab, TabContext, TabOpenActions, TabRouteAdapter, TabTarget } from './types';
import { terminalRoute } from './terminal';
import { fileRoute, filesRoute } from './files';
import { diffRoute } from './diff';
import { prRoute } from './pr';
import { sessionRoute } from './session';
import { automationRoute } from './automation';
import { prefix } from './helpers';
export type { TabTarget, ResolvedTab, TabOpenActions, AutomationLocation } from './types';

// The registry is the only dispatch point; each adapter owns its typed payload.
const adapters = [terminalRoute, filesRoute, fileRoute, diffRoute, prRoute, sessionRoute, automationRoute] as TabRouteAdapter[];
const registry = new Map(adapters.map(adapter => [adapter.type, adapter]));
export function parseTabUrl(url: URL): TabTarget | undefined {
  if (!url.pathname.startsWith(prefix)) return;
  const parts = url.pathname.slice(prefix.length).replace(/\/$/, '').split('/').map(decodeURIComponent);
  const adapter = registry.get(parts[0] as TabTarget['type']);
  if (adapter) return adapter.parse(parts, url.searchParams);
  if (parts.length === 1 && parts[0]) return { type: 'terminal', tabId: parts[0] }; // Legacy notification links.
  throw new Error('无法识别此 Tab 链接。');
}
export const tabTargetUrl = (target: TabTarget) => registry.get(target.type)!.url(target);
export const aowTabUrl = (tabId: string) => tabTargetUrl({ type: 'terminal', tabId });
export function captureTabTarget(context: TabContext) {
  for (const adapter of adapters) {
    const target = adapter.capture(context);
    if (target) return target;
  }
}
export async function resolveTabTarget(target: TabTarget, signal: AbortSignal): Promise<ResolvedTab> {
  const terminal = target.type === 'terminal' ? await terminalApi.get(target.tabId, signal) : undefined;
  const projects = await aowApi.projects();
  signal.throwIfAborted();
  const contexts = projects.flatMap(project => project.worktrees.map(worktree => ({ project, worktree })));
  const context = contexts.find(({ worktree }) => terminal ? worktree.path === terminal.workspace_root : worktree.id === (target as Exclude<TabTarget, { type: 'terminal' }>).workspace);
  if (!context) throw new Error('目标 Tab 所属工作区不存在或已被移除。');
  const resolved = await registry.get(target.type)!.resolve(target, context, signal);
  signal.throwIfAborted();
  return { target, workspacePath: context.worktree.path, terminal, ...resolved };
}
export const openTabTarget = (resolved: ResolvedTab, actions: TabOpenActions) =>
  registry.get(resolved.target.type)!.open(resolved.target, resolved, actions);
export function tabLocationUrl(target?: TabTarget, hash = '', search = window.location.search) {
  const url = new URL(target ? tabTargetUrl(target) : appUrl('/aow/'), window.location.origin);
  const existing = new URLSearchParams(search);
  // UI mode is page-level; resource parameters always come from the active Tab.
  if (existing.has('ui')) url.searchParams.set('ui', existing.get('ui')!);
  url.hash = hash;
  return url.pathname + url.search + url.hash;
}


export const tabCenterId = (target: TabTarget) => registry.get(target.type)!.centerId(target);
