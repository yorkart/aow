import type { TabRouteAdapter, TabTarget } from './types';
import { length, segment, url } from './helpers';
export const terminalRoute: TabRouteAdapter<Extract<TabTarget, { type: 'terminal' }>> = {
  type: 'terminal',
  centerId: t => 'terminal:' + t.tabId,
  parse(parts) { length(parts, 2); return { type: 'terminal', tabId: segment(parts, 1) }; },
  url: target => url('terminal/' + encodeURIComponent(target.tabId), {}),
  capture: ({ tab }) => tab?.id.startsWith('terminal:') ? { type: 'terminal', tabId: tab.targetId } : undefined,
  resolve: async () => ({}),
  open: (target, _, actions) => actions.terminal(target.tabId),
};

