import { automationApi } from '../../features/automations/api';
import type { TabRouteAdapter, TabTarget } from './types';
import { length, required, segment, url } from './helpers';
export const automationRoute: TabRouteAdapter<Extract<TabTarget, { type: 'automation' }>> = {
  type: 'automation',
  centerId: t => 'automation:' + t.taskId,
  parse(parts, q) {
    if (parts.length === 4 && parts[2] === 'runs') segment(parts, 3); else length(parts, 2);
    return { type: 'automation', workspace: required(q, 'workspace'), taskId: segment(parts, 1),
      runId: parts[3], view: parts[3] || q.get('view') === 'runs' ? 'runs' : undefined };
  },
  url: t => url('automation/' + encodeURIComponent(t.taskId) + (t.runId ? '/runs/' + encodeURIComponent(t.runId) : ''),
    { workspace: t.workspace, view: !t.runId ? t.view : undefined }),
  capture(c) {
    if (!c.tab?.id.startsWith('automation:')) return;
    const location = c.automationLocations[c.tab.targetId];
    return { type: 'automation', workspace: c.workspace, taskId: c.tab.targetId,
      runId: location?.runId, view: location?.view === 'runs' ? 'runs' : undefined };
  },
  async resolve(t, context) {
    const task = await automationApi.detail(t.taskId);
    if (task.project_id !== context.project.id) throw new Error('自动化任务不属于该工作区的项目。');
    if (t.runId) await automationApi.runDetail(t.taskId, t.runId);
    return { task };
  },
  open: (t, resolved, actions) => actions.automation(resolved.task!, { view: t.view ?? 'overview', runId: t.runId }),
};

