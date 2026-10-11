import { useCallback, useEffect, useRef, useState } from 'react';
import { aowApi } from './aowApi';
import type { WorktreeCreationJob } from './types';
import { operationsChangedEvent } from '../features/operations/operations';

export const creationActive = (job: Pick<WorktreeCreationJob, 'status'>) => job.status === 'pending' || job.status === 'running';

export function useWorktreeCreations() {
  const [jobs, setJobs] = useState<WorktreeCreationJob[]>([]);
  const [error, setError] = useState('');
  const mounted = useRef(false);
  const generation = useRef(0);
  const pending = useRef(false);
  const refresh = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    const version = generation.current;
    try {
      const result = await aowApi.worktreeCreations();
      if (mounted.current && generation.current === version) { setJobs(result); setError(''); }
    } catch (reason) {
      if (mounted.current) setError(`无法更新创建进度：${reason instanceof Error ? reason.message : String(reason)}`);
    } finally { pending.current = false; }
  }, []);
  const submitted = useCallback((job: WorktreeCreationJob) => {
    generation.current += 1;
    setJobs(current => [...current.filter(item => item.id !== job.id), job]);
    setError('');
  }, []);
  const active = jobs.some(creationActive);
  useEffect(() => {
    mounted.current = true;
    void refresh();
    const visible = () => { if (document.visibilityState === 'visible') void refresh(); };
    document.addEventListener('visibilitychange', visible);
    window.addEventListener('focus', visible);
    window.addEventListener(operationsChangedEvent, visible);
    return () => {
      mounted.current = false;
      document.removeEventListener('visibilitychange', visible);
      window.removeEventListener('focus', visible);
      window.removeEventListener(operationsChangedEvent, visible);
    };
  }, [refresh]);
  useEffect(() => {
    const timer = window.setInterval(() => { if (document.visibilityState === 'visible') void refresh(); }, active || error ? 1500 : 15000);
    return () => window.clearInterval(timer);
  }, [active, error, refresh]);
  return { jobs, error, submitted, refresh };
}
