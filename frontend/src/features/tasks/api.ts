import { useEffect, useMemo, useSyncExternalStore } from 'react';
import { aowRequest } from '../../lib/aowRequest';
import { subscribeWorkspaceChanges } from '../../lib/workspaceEvents';
import type { BoardTask, ConvertInput, InboxItem, InboxPage, InboxSummary, TaskBoardData, TaskStatus } from './types';

export const taskError = (error: unknown) => error instanceof Error ? error.message : String(error);

class Feed<T> {
  snapshot: { data?: T; error?: string; loading: boolean } = { loading: false };
  listeners = new Set<() => void>();
  pending?: Promise<void>;
  again = false;
  constructor(readonly fetch: () => Promise<T>) {}
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  getSnapshot = () => this.snapshot;
  emit = () => this.listeners.forEach(listener => listener());
  reload = (): Promise<void> => {
    if (this.pending) { this.again = true; return this.pending; }
    this.pending = (async () => {
      do {
        this.again = false;
        this.snapshot = { ...this.snapshot, loading: true }; this.emit();
        try { this.snapshot = { data: await this.fetch(), loading: false }; }
        catch (error) { this.snapshot = { ...this.snapshot, error: taskError(error), loading: false }; }
        this.emit();
      } while (this.again);
    })().finally(() => { this.pending = undefined; });
    return this.pending;
  };
}

const boards = new Map<string, Feed<TaskBoardData>>();
const inboxes = new Map<string, { feed: Feed<InboxPage>; pages: number }>();
function boardFeed(projectId: string) {
  let feed = boards.get(projectId);
  if (!feed) {
    feed = new Feed(() => aowRequest<TaskBoardData>(`/api/tasks?${new URLSearchParams({ project_id: projectId })}`));
    boards.set(projectId, feed);
  }
  return feed;
}
function inboxFeed(projectId: string, includeConverted: boolean) {
  const key = JSON.stringify([projectId, includeConverted]);
  let entry = inboxes.get(key);
  if (!entry) {
    const value = { pages: 1, feed: new Feed<InboxPage>(async () => {
      const items: InboxSummary[] = [];
      let cursor: string | null = null;
      let page: InboxPage;
      for (let index = 0; index < value.pages; index++) {
        const query = new URLSearchParams({ project_id: projectId, include_converted: String(includeConverted), limit: '50' });
        if (cursor) query.set('cursor', cursor);
        page = await aowRequest<InboxPage>(`/api/tasks/inbox?${query}`);
        items.push(...page.items); cursor = page.next_cursor;
        if (!cursor || index === value.pages - 1) return { ...page, items };
      }
      throw new Error('Invalid Inbox page count');
    }) };
    entry = value; inboxes.set(key, value);
  }
  return entry;
}

export async function refreshTasks(): Promise<void> {
  await Promise.all([...boards.values(), ...[...inboxes.values()].map(entry => entry.feed)]
    .filter(feed => feed.listeners.size).map(feed => feed.reload()));
}
async function write<T>(path: string, value: unknown) {
  try { return await aowRequest<T>(`/api/tasks${path}`, { method: 'POST', body: JSON.stringify(value) }); }
  finally { void refreshTasks(); }
}
export const tasksApi = {
  inbox: (input: { id: string; project_id: string; expected_revision?: number; title: string; description: string }) => write<InboxItem>('/inbox', input),
  getInbox: (id: string) => aowRequest<InboxItem>(`/api/tasks/inbox/${encodeURIComponent(id)}`),
  deleteInbox: (item: InboxSummary) => write(`/inbox/${encodeURIComponent(item.id)}/delete`, { expected_revision: item.revision }),
  convert: (item: InboxItem, input: ConvertInput) => write<BoardTask>(`/inbox/${encodeURIComponent(item.id)}/convert`, input),
  statuses: (input: { statuses: TaskStatus[]; expected_revision: number }) => write<TaskBoardData>('/statuses', input),
  move: (task: BoardTask, status: string) => write<BoardTask>(`/items/${encodeURIComponent(task.id)}/status`, { expected_revision: task.revision, status_id: status, reason: 'Updated on task board' }),
  start: (task: BoardTask) => write<BoardTask>(`/items/${encodeURIComponent(task.id)}/start`, { expected_revision: task.revision }),
  archive: (task: BoardTask) => write<BoardTask>(`/items/${encodeURIComponent(task.id)}/archive`, { expected_revision: task.revision }),
};
export function useTaskBoard(projectId: string, visible: boolean) {
  const feed = useMemo(() => boardFeed(projectId), [projectId]);
  const state = useSyncExternalStore(feed.subscribe, feed.getSnapshot);
  useEffect(() => {
    if (!visible) return;
    void feed.reload();
    const unsubscribe = subscribeWorkspaceChanges(change => { if (change.reset || change.tasks) void refreshTasks(); });
    const focus = () => { if (document.visibilityState === 'visible') void refreshTasks(); };
    document.addEventListener('visibilitychange', focus);
    return () => { unsubscribe(); document.removeEventListener('visibilitychange', focus); };
  }, [feed, visible]);
  return { board: state.data, error: state.error, projectId, visible };
}
export function useInboxPage(projectId: string, includeConverted: boolean, visible: boolean) {
  const entry = useMemo(() => inboxFeed(projectId, includeConverted), [projectId, includeConverted]);
  const state = useSyncExternalStore(entry.feed.subscribe, entry.feed.getSnapshot);
  useEffect(() => { if (visible) void entry.feed.reload(); }, [entry, visible]);
  return { ...state, loadMore: () => { if (!entry.feed.pending) { entry.pages++; void entry.feed.reload(); } } };
}
