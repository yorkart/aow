import type { TabRouteAdapter, TabTarget } from './types';
import { length, required, url } from './helpers';
export const tasksRoute: TabRouteAdapter<Extract<TabTarget, { type: 'tasks' }>> = {
  type: 'tasks', centerId: () => 'task-board',
  parse(parts, query) { length(parts, 1); return { type: 'tasks', workspace: required(query, 'workspace') }; },
  url: target => url('tasks', { workspace: target.workspace }),
  capture: context => context.tab?.kind === 'tasks' ? { type: 'tasks', workspace: context.workspace } : undefined,
  resolve: async () => ({}),
  open: (_target, _resolved, actions) => actions.tasks(),
};
