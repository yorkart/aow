import { aowRequest } from '../../lib/aowRequest';
import type { InboxComment, InboxExecuteOptions, InboxExecution, InboxItem, InboxLabel, InboxSnapshot } from './types';
const base = '/api/inbox';
const itemPath = (id: string) => `${base}/items/${encodeURIComponent(id)}`;
const json = (method: string, value: unknown) => ({ method, body: JSON.stringify(value) });
export const inboxApi = {
  list: () => aowRequest<InboxSnapshot>(base, { cache: 'no-store' }),
  comments: (id: string) => aowRequest<InboxComment[]>(`${itemPath(id)}/comments`, { cache: 'no-store' }),
  capture: (markdown: string, requestKey: string) => aowRequest<InboxItem>(`${base}/items`, json('POST', { markdown, request_key: requestKey })),
  update: (item: InboxItem, patch: Partial<Pick<InboxItem, 'markdown' | 'project_id' | 'label_ids'>>) => aowRequest<InboxItem>(itemPath(item.id), json('PUT', {
    expected_revision: item.revision, markdown: item.markdown, project_id: item.project_id, label_ids: item.label_ids, ...patch,
  })),
  remove: (item: InboxItem) => aowRequest<void>(itemPath(item.id), json('DELETE', { expected_revision: item.revision })),
  reorder: (revision: number, itemId: string, beforeId: string | null) => aowRequest<void>(`${base}/order`, json('PUT', { expected_revision: revision, item_id: itemId, before_id: beforeId })),
  labels: (revision: number, labels: InboxLabel[]) => aowRequest<void>(`${base}/labels`, json('PUT', { expected_revision: revision, labels })),
  execute: (item: InboxItem, options: InboxExecuteOptions, requestKey: string) => aowRequest<InboxExecution>(`${itemPath(item.id)}/execute`, json('POST', {
    expected_revision: item.revision, request_key: requestKey, ...options,
  })),
};
