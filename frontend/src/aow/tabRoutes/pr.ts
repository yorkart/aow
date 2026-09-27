import { prApi } from '../../features/pr/api';
import type { ReviewIdentity } from '../../features/pr/types';
import type { TabRouteAdapter, TabTarget } from './types';
import { absolutePath, length, required, segment, url } from './helpers';
export const prTabId = (repository: string, number: number, target?: ReviewIdentity) =>
  'pull-request:' + encodeURIComponent(repository) + ':' + number +
  (target?.provider && target.provider !== 'auto' || target?.remote ? ':' + encodeURIComponent(target?.provider ?? 'auto') + ':' + encodeURIComponent(target?.remote ?? '') : '');
export const prRoute: TabRouteAdapter<Extract<TabTarget, { type: 'pr' }>> = {
  type: 'pr',
  centerId: t => prTabId(t.repository, t.number, t),
  parse(parts, q) {
    length(parts, 3);
    const provider = segment(parts, 1);
    const number = Number(segment(parts, 2));
    if (!Number.isSafeInteger(number) || number <= 0) throw new Error('PR 编号无效。');
    return { type: 'pr', workspace: required(q, 'workspace'), repository: absolutePath(required(q, 'repository')), provider, number, remote: q.get('remote') || undefined };
  },
  url: t => url('pr/' + encodeURIComponent(t.provider) + '/' + t.number, { workspace: t.workspace, repository: t.repository, remote: t.remote }),
  capture(c) {
    if (c.tab?.kind !== 'pullRequest') return;
    const pr = c.pullRequests.find(r => prTabId(r.repository ?? c.workspacePath, r.number, r) === c.tab?.id);
    return pr ? { type: 'pr', workspace: c.workspace, repository: pr.repository ?? c.workspacePath, provider: pr.provider ?? 'auto', remote: pr.remote, number: pr.number } : undefined;
  },
  async resolve(t) {
    const pr = await prApi.myPullRequest(t.repository, t.number, t);
    return { target: { ...t, provider: pr.provider ?? t.provider, remote: pr.remote ?? t.remote }, pullRequest: { ...pr, provider: pr.provider ?? t.provider, remote: pr.remote ?? t.remote } };
  },
  open: (t, resolved, actions) => actions.pullRequest(resolved.pullRequest!, t.repository),
};
