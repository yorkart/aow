import { resolveAcpSession } from '../../features/acp/workspace';
import type { TabRouteAdapter, TabTarget } from './types';
import { length, segment, url } from './helpers';

export const acpRoute: TabRouteAdapter<Extract<TabTarget, { type: 'acp' }>> = {
  type: 'acp',
  centerId: t => 'acp:' + t.thread_id,
  parse(parts) {
    length(parts, 2);
    return { type: 'acp', thread_id: segment(parts, 1) };
  },
  url: t => url('acp/' + encodeURIComponent(t.thread_id), {}),
  capture(context) {
    const tab = context.acpTabs.find(tab => context.tab?.id === 'acp:' + tab.id);
    return tab?.thread_id ? { type: 'acp', thread_id: tab.thread_id } : undefined;
  },
  async locate(target, signal) {
    const { session, tabId } = await resolveAcpSession(target.thread_id, signal);
    return { workspacePath: session.cwd, acpSession: session, centerId: 'acp:' + tabId };
  },
  resolve: async () => ({}),
  open: (_, resolved, actions) => actions.acp(resolved.acpSession!),
};
