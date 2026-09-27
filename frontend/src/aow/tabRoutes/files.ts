import type { DocumentSource } from '../../features/editor/types';
import type { TabRouteAdapter, TabTarget } from './types';
import { absolutePath, length, required, url } from './helpers';
const sources = new Set<DocumentSource>(['project', 'notes', 'external']);
export const filesRoute: TabRouteAdapter<Extract<TabTarget, { type: 'files' }>> = {
  type: 'files',
  centerId: () => 'system-files',
  parse(parts, q) { length(parts, 1); return { type: 'files', workspace: required(q, 'workspace'), path: absolutePath(required(q, 'path')) }; },
  url: t => url('files', { workspace: t.workspace, path: t.path }),
  capture: c => c.tab?.kind === 'browser' && c.browserPath ? { type: 'files', workspace: c.workspace, path: c.browserPath } : undefined,
  resolve: async () => ({}),
  open: (t, _, actions) => actions.files(t.path),
};
export const fileRoute: TabRouteAdapter<Extract<TabTarget, { type: 'file' }>> = {
  type: 'file',
  centerId: t => 'document:' + t.path,
  parse(parts, q) {
    length(parts, 1);
    const source = (q.get('source') ?? 'project') as DocumentSource;
    if (!sources.has(source)) throw new Error('无法识别文件来源。');
    return { type: 'file', workspace: required(q, 'workspace'), path: absolutePath(required(q, 'path')), source };
  },
  url: t => url('file', { workspace: t.workspace, path: t.path, source: t.source }),
  capture(c) {
    const document = c.documents.find(d => c.tab?.id === 'document:' + d.id);
    return document && !document.diffSource ? { type: 'file', workspace: c.workspace, path: document.path, source: document.explorerSource ?? 'external' } : undefined;
  },
  resolve: async () => ({}),
  open: (t, _, actions) => actions.file(t.path, t.source),
};

