import type { OpenDocument } from '../../features/editor/types';
import type { TabRouteAdapter, TabTarget } from './types';
import { absolutePath, length, required, url } from './helpers';
export const diffRoute: TabRouteAdapter<Extract<TabTarget, { type: 'diff' }>> = {
  type: 'diff',
  centerId: t => 'document:' + diffDocument(t).id,
  parse(parts, q) {
    length(parts, 1);
    const source = required(q, 'source');
    if (source !== 'working' && source !== 'staged' && source !== 'commit') throw new Error('无法识别 Diff 来源。');
    const commit = source === 'commit' ? required(q, 'commit') : undefined;
    if (commit && !/^[a-f0-9]{40,64}$/i.test(commit)) throw new Error('Commit Diff 必须使用完整提交 SHA。');
    return { type: 'diff', workspace: required(q, 'workspace'), repository: absolutePath(required(q, 'repository')),
      path: required(q, 'path'), source, commit, originalPath: q.get('original_path') ?? undefined, untracked: q.get('untracked') === '1' || undefined };
  },
  url: t => url('diff', { workspace: t.workspace, repository: t.repository, path: t.path, source: t.source,
    commit: t.commit, original_path: t.originalPath, untracked: t.untracked ? '1' : undefined }),
  capture(c) {
    const document = c.documents.find(d => c.tab?.id === 'document:' + d.id);
    const source = document?.diffSource;
    if (!source || !document) return;
    return { type: 'diff', workspace: c.workspace, repository: source.repository, path: document.path,
      ...(source.kind === 'commit' ? { source: 'commit' as const, commit: source.commit, originalPath: source.originalPath }
        : { source: source.staged ? 'staged' as const : 'working' as const, untracked: source.untracked || undefined }) };
  },
  resolve: async () => ({}),
  open: (t, _, actions) => actions.diff(t),
};


export function diffDocument(t: Extract<TabTarget, { type: 'diff' }>): OpenDocument {
  const source: OpenDocument['diffSource'] = t.source === 'commit'
    ? { kind: 'commit', repository: t.repository, commit: t.commit!, originalPath: t.originalPath }
    : { kind: 'working', repository: t.repository, staged: t.source === 'staged', untracked: !!t.untracked };
  return { id: t.source === 'commit' ? `commit-diff:${t.repository}:${t.commit}:${t.path}` : `diff:${t.repository}:${t.source === 'staged'}:${t.path}`,
    path: t.path, name: `${t.path.split('/').pop()} (${t.source === 'commit' ? t.commit!.slice(0, 8) : 'Diff'})`, kind: 'diff', readOnly: true, loading: true, diffSource: source };
}
