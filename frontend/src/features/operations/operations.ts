import { appUrl } from '../../lib/basePath';
import { useCallback, useEffect, useRef, useState } from 'react';

export type OperationOutcome = 'succeeded' | 'partial_success' | 'failed' | 'interrupted' | 'cancelled';
export interface Operation {
  id: string;
  kind: string;
  source: string;
  title: string;
  project_id: string | null;
  resource: string | null;
  created_at: string;
  message: string;
  completed: number;
  total: number | null;
  outcome: OperationOutcome | null;
}
export interface OperationSnapshot { boot_id: string; revision: number; operations: Operation[]; log_error: string | null }
export interface LogCursor { file: string; offset: number }
export interface OperationLog {
  timestamp: string; operation_id: string; boot_id: string; kind: string; source: string; title: string;
  event: string; level: 'info' | 'warn' | 'error'; message: string;
  project_id?: string; resource?: string; outcome?: OperationOutcome; completed?: number; total?: number;
}
export interface OperationLogPage { items: OperationLog[]; next_cursor: LogCursor | null; budget_exhausted: boolean; scanned_bytes: number }
export const operationOutcomeLabels: Record<OperationOutcome, string> = {
  succeeded: '已完成', partial_success: '部分失败', failed: '失败', interrupted: '结果未确认', cancelled: '已取消',
};
export const operationsChangedEvent = 'aow-operations-changed';

async function request<T>(url: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(appUrl(url), { cache: 'no-store', signal });
  if (!response.ok) {
    const error = await response.json().catch(() => null);
    throw new Error(error?.message ?? `请求失败 (${response.status})`);
  }
  return response.json() as Promise<T>;
}

export function readOperationLogs(filters: Record<string, string>, cursor: LogCursor | undefined, signal: AbortSignal) {
  const query = new URLSearchParams({ limit: '50' });
  for (const [key, value] of Object.entries(filters)) if (value) query.set(key, value);
  if (cursor) query.set('cursor', JSON.stringify(cursor));
  return request<OperationLogPage>(`/api/operation-logs?${query}`, signal);
}

export function useOperations() {
  const [snapshot, setSnapshot] = useState<OperationSnapshot>({ boot_id: '', revision: 0, operations: [], log_error: null });
  const [error, setError] = useState('');
  const refreshRef = useRef<() => void>(() => {});
  const refresh = useCallback(() => refreshRef.current(), []);
  useEffect(() => {
    let mounted = true;
    let generation = 0;
    let latest: OperationSnapshot | undefined;
    let pending = false;
    const controller = new AbortController();
    const apply = (next: OperationSnapshot) => {
      if (!mounted || !Array.isArray(next.operations)) return;
      if (latest?.boot_id === next.boot_id && latest.revision > next.revision) return;
      const changed = !latest || latest.boot_id !== next.boot_id || latest.revision !== next.revision;
      latest = next;
      setSnapshot(next);
      setError('');
      if (changed) window.dispatchEvent(new Event(operationsChangedEvent));
    };
    const reload = async () => {
      if (pending || !mounted) return;
      pending = true;
      const version = generation;
      try {
        const next = await request<OperationSnapshot>('/api/operations/active', controller.signal);
        if (generation === version) apply(next);
      } catch (reason) {
        if (mounted) setError(reason instanceof Error ? reason.message : String(reason));
      } finally { pending = false; }
    };
    refreshRef.current = () => { void reload(); };
    void reload();
    const source = new EventSource(appUrl('/api/operations/stream'));
    const receive = (event: MessageEvent<string>) => {
      try { const next = JSON.parse(event.data) as OperationSnapshot; generation += 1; apply(next); } catch { /* A reconnect obtains a fresh snapshot. */ }
    };
    source.addEventListener('operations', receive);
    source.onerror = () => { void reload(); };
    const visible = () => { if (document.visibilityState === 'visible') void reload(); };
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible' && (source.readyState !== EventSource.OPEN || latest?.operations.some(item => !item.outcome))) void reload();
    }, 1500);
    document.addEventListener('visibilitychange', visible);
    window.addEventListener('focus', visible);
    return () => {
      mounted = false;
      controller.abort();
      source.close();
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', visible);
      window.removeEventListener('focus', visible);
      refreshRef.current = () => {};
    };
  }, []);
  return { ...snapshot, error, refresh };
}
