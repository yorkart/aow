import { storageKey } from '../../lib/basePath';
import { useMemo, useSyncExternalStore } from 'react';

interface TaskStopSource {
  project_name: string;
  workspace_root: string;
  tab_id: string;
  tab_name: string;
}

export interface TaskStop {
  agent: string;
  session_id: string;
  title: string;
  cwd: string;
  sources: TaskStopSource[];
}

export interface TaskNotice extends TaskStop {
  key: number;
  receivedAt: number;
  popupClosed?: boolean;
}

// Persist only the small unread notification payload, never the full task conclusion.
export function parseTaskStop(value: unknown): TaskStop | undefined {
  if (!value || typeof value !== 'object') return;
  const data = value as Partial<TaskStop>;
  if (typeof data.agent !== 'string' || typeof data.session_id !== 'string'
    || typeof data.title !== 'string' || typeof data.cwd !== 'string') return;
  const sources = (Array.isArray(data.sources) ? data.sources : []).flatMap(source => {
    if (!source || typeof source.project_name !== 'string' || typeof source.workspace_root !== 'string'
      || typeof source.tab_id !== 'string' || typeof source.tab_name !== 'string') return [];
    return [{ project_name: source.project_name, workspace_root: source.workspace_root,
      tab_id: source.tab_id, tab_name: source.tab_name }];
  });
  return { agent: data.agent, session_id: data.session_id, title: data.title, cwd: data.cwd, sources };
}

const storeName = 'pending';
function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(storageKey('aow-task-notifications'), 1);
    request.onupgradeneeded = () => request.result.createObjectStore(storeName, { keyPath: 'key', autoIncrement: true });
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () => reject(new Error('Notification storage is blocked'));
  });
}

function transaction<T>(database: IDBDatabase, mode: IDBTransactionMode, action: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const tx = database.transaction(storeName, mode);
    const request = action(tx.objectStore(storeName));
    // Wait for the commit, so a notice never disappears before its deletion is durable.
    tx.oncomplete = () => resolve(request.result);
    tx.onabort = () => reject(tx.error ?? request.error);
    tx.onerror = () => reject(tx.error ?? request.error);
  });
}

function createNotificationQueue() {
  let snapshot: { notices: TaskNotice[]; storageError: string } = { notices: [], storageError: '' };
  const listeners = new Set<() => void>();
  let database: IDBDatabase | undefined;
  let operations: Promise<void> | undefined;
  let memoryKey = 0;
  const publish = (notices = snapshot.notices, storageError = snapshot.storageError) => {
    snapshot = { notices, storageError };
    for (const listener of listeners) listener();
  };
  const initialize = () => {
    if (!operations) operations = openDatabase().then(async db => {
      database = db;
      db.onversionchange = () => { db.close(); database = undefined; };
      const saved: unknown[] = await transaction(db, 'readonly', store => store.getAll());
      const notices = saved.flatMap(value => {
        const data = parseTaskStop(value);
        const notice = value as Partial<TaskNotice> | null;
        return data && typeof notice?.key === 'number' && typeof notice.receivedAt === 'number'
          ? [{ ...data, key: notice.key, receivedAt: notice.receivedAt, popupClosed: notice.popupClosed === true }] : [];
      });
      publish(notices.reverse(), '');
    }).catch(() => publish(undefined, '通知暂时无法保存，刷新页面后可能丢失。'));
    return operations;
  };
  const enqueue = (operation: () => Promise<void>) => {
    // Loading, incoming events and dismissals share one queue; restoration cannot
    // overwrite a new event or resurrect a notice dismissed while loading.
    operations = initialize().then(operation).catch(() => publish(undefined, '通知保存失败，请重试。'));
  };
  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      void initialize();
      return () => { listeners.delete(listener); };
    },
    getSnapshot: () => snapshot,
    add(data: TaskStop) {
      const receivedAt = Date.now();
      enqueue(async () => {
        let key = --memoryKey;
        let storageError = snapshot.storageError;
        try {
          if (database) key = Number(await transaction(database, 'readwrite', store => store.add({ ...data, receivedAt })));
          else storageError = '通知暂时无法保存，刷新页面后可能丢失。';
        } catch { storageError = '通知暂时无法保存，刷新页面后可能丢失。'; }
        publish([{ ...data, receivedAt, key }, ...snapshot.notices], storageError);
      });
    },
    closePopups(matches: (notice: TaskNotice) => boolean) {
      enqueue(async () => {
        const closed = snapshot.notices.filter(notice => !notice.popupClosed && matches(notice))
          .map(notice => ({ ...notice, popupClosed: true }));
        if (!closed.length) return;
        if (database) await transaction(database, 'readwrite', store => {
          for (const notice of closed) if (notice.key > 0) store.put(notice);
          return store.count();
        });
        else if (closed.some(notice => notice.key > 0)) throw new Error('Notification storage is unavailable');
        const updates = new Map(closed.map(notice => [notice.key, notice]));
        // Closing a popup leaves the notice unread until its source tab is activated.
        publish(snapshot.notices.map(notice => updates.get(notice.key) ?? notice));
      });
    },
    dismiss(matches: (notice: TaskNotice) => boolean) {
      enqueue(async () => {
        const removed = snapshot.notices.filter(matches);
        if (!removed.length) return;
        if (database) await transaction(database, 'readwrite', store => {
          for (const notice of removed) if (notice.key > 0) store.delete(notice.key);
          return store.count();
        });
        else if (removed.some(notice => notice.key > 0)) throw new Error('Notification storage is unavailable');
        const keys = new Set(removed.map(notice => notice.key));
        publish(snapshot.notices.filter(notice => !keys.has(notice.key)));
      });
    },
  };
}

export const taskNotifications = createNotificationQueue();
export const useTaskNotifications = () => useSyncExternalStore(taskNotifications.subscribe, taskNotifications.getSnapshot);

export function useWorktreeUnreadCounts() {
  const { notices } = useTaskNotifications();
  return useMemo(() => {
    const counts = new Map<string, number>();
    for (const notice of notices) {
      // Several source tabs in one worktree still represent a single notice.
      const roots = new Set(notice.sources.map(source => source.workspace_root).filter(Boolean));
      for (const root of roots) counts.set(root, (counts.get(root) ?? 0) + 1);
    }
    return counts;
  }, [notices]);
}
