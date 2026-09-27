import { useCallback, useEffect, useRef, useState } from 'react';
import { aowApi } from './aowApi';
import type { WorktreeRemovalItem, WorktreeRemovalJob } from './types';
import { operationsChangedEvent } from '../features/operations/operations';

export const removalActive = (item: WorktreeRemovalItem) => item.status === 'queued' || item.status === 'running';
export const removalStatusLabel: Record<WorktreeRemovalItem['status'], string> = {
  queued: '排队中', running: '删除中', succeeded: '已删除', failed: '删除失败', interrupted: '已中断',
};

export function useWorktreeRemovals() {
  const [jobs, setJobs] = useState<WorktreeRemovalJob[]>([]);
  const [error, setError] = useState('');
  const mounted = useRef(false);
  const generation = useRef(0);
  const pending = useRef(false);
  const tracked = useRef(new Set<string>());
  const refresh = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    const version = generation.current;
    try {
      const result = await aowApi.worktreeRemovals();
      if (mounted.current && generation.current === version) {
        const current = result.filter(job => tracked.current.has(job.id) || job.items.some(removalActive));
        tracked.current = new Set(current.map(job => job.id));
        setJobs(current);
        setError('');
      }
    } catch (reason) {
      if (mounted.current) setError(`无法更新清理进度：${reason instanceof Error ? reason.message : String(reason)}`);
    } finally { pending.current = false; }
  }, []);
  const submitted = useCallback((job: WorktreeRemovalJob) => {
    generation.current += 1;
    tracked.current.add(job.id);
    setJobs(current => [...current.filter(item => item.id !== job.id), job]);
    setError('');
  }, []);
  const dismiss = useCallback((projectId: string) => {
    generation.current += 1;
    setJobs(current => {
      const next = current.filter(job => job.project_id !== projectId || job.items.some(removalActive));
      tracked.current = new Set(next.map(job => job.id));
      return next;
    });
  }, []);
  const active = jobs.some(job => job.items.some(removalActive));
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const onVisible = () => { if (document.visibilityState === 'visible') void refresh(); };
    document.addEventListener('visibilitychange', onVisible);
    window.addEventListener('focus', onVisible);
    window.addEventListener(operationsChangedEvent, onVisible);
    return () => {
      mounted.current = false;
      document.removeEventListener('visibilitychange', onVisible);
      window.removeEventListener('focus', onVisible);
      window.removeEventListener(operationsChangedEvent, onVisible);
    };
  }, [refresh]);
  useEffect(() => {
    // Also retry a failed initial request, and discover tasks from other tabs.
    const timer = window.setInterval(() => { if (document.visibilityState === 'visible') void refresh(); }, active || error ? 1500 : 15000);
    return () => window.clearInterval(timer);
  }, [active, error, refresh]);
  return { jobs, error, refresh, submitted, dismiss };
}
