import { acpRequest } from '../acp/request';
import { subscribeWorkspaceChanges } from '../../lib/workspaceEvents';
import { useEffect, useState } from 'react';

export interface TaskRun {
  id: string; task_id: string; project_id: string; thread_id: string; prompt: string;
  state: 'queued' | 'running' | 'waiting' | 'result_available' | 'failed' | 'interrupted';
  result: string | null; error: string | null; created_at: number; finished_at: number | null;
  source: { task_id: string; thread_id: string; turn_id: string | null; message_id: string | null; request: string } | null;
}
export interface InboxEvent {
  id: string; run: TaskRun; read: boolean; archived: boolean; created_at: number;
  delivery: { state: 'pending' | 'admitted' | 'processed' | 'blocked'; error: string | null } | null;
}
export interface InboxPage { items: InboxEvent[]; unread: number; has_more: boolean }
const changed = 'aow:inbox-changed';
export function subscribeInbox(listener: () => void) {
  const unsubscribe = subscribeWorkspaceChanges(change => { if (change.reset || change.inbox) listener(); });
  window.addEventListener(changed, listener);
  return () => { unsubscribe(); window.removeEventListener(changed, listener); };
}
async function mutate(id: string, action: 'update' | 'retry', body?: unknown) {
  await acpRequest<void>('/api/inbox/' + encodeURIComponent(id) + (action === 'retry' ? '/retry' : ''),
    { method: action === 'retry' ? 'POST' : 'PATCH', body: body === undefined ? undefined : JSON.stringify(body) });
  window.dispatchEvent(new Event(changed));
}
export const inboxApi = {
  list: (params: URLSearchParams, signal?: AbortSignal) => acpRequest<InboxPage>('/api/inbox?' + params, { signal, cache: 'no-store' }),
  update: (id: string, body: { read?: boolean; archived?: boolean }) => mutate(id, 'update', body),
  retry: (id: string) => mutate(id, 'retry'),
};
export function useInboxUnread() {
  const [count, setCount] = useState(0);
  useEffect(() => {
    let controller: AbortController | undefined;
    const load = () => {
      controller?.abort(); controller = new AbortController();
      const current = controller;
      void inboxApi.list(new URLSearchParams({ limit: '1' }), current.signal)
        .then(page => { if (!current.signal.aborted) setCount(page.unread); }).catch(() => {});
    };
    load();
    const unsubscribe = subscribeInbox(load);
    return () => { controller?.abort(); unsubscribe(); };
  }, []);
  return count;
}
