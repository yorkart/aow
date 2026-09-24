import { useCallback, useEffect, useRef } from 'react';
import { gitApi } from './api';
import type { GitLog, GitStatus } from './types';

type RefreshTarget = 'all' | 'changes' | 'commits';
interface Callbacks {
  status: (status: GitStatus) => void;
  log: (log: GitLog) => void;
  error: (message: string) => void;
}

// One round per visible panel. Manual requests and save notifications share
// the same queue; a slow round never accumulates interval ticks.
export function useSourceControlPolling(repository: string, visible: boolean, generation: number, callbacks: Callbacks) {
  const latest = useRef({ repository, visible, callbacks });
  latest.current = { repository, visible, callbacks };
  const refreshRef = useRef<(mask: number) => void>(() => {});
  const previousGeneration = useRef(generation);

  useEffect(() => {
    if (!repository || !visible) return;
    let disposed = false;
    let timer: number | undefined;
    let controller: AbortController | undefined;
    let pending = 0;
    const errors = new Map<number, string>();
    const active = () => !disposed && latest.current.visible
      && latest.current.repository === repository && document.visibilityState === 'visible';
    const clearTimer = () => { window.clearTimeout(timer); timer = undefined; };
    const refresh = async (mask: number) => {
      if (!active()) return;
      clearTimer();
      if (controller) { pending |= mask; return; }
      const request = new AbortController();
      controller = request;
      const run = async <T,>(key: number, load: () => Promise<T>, apply: (value: T) => void) => {
        try {
          const result = await load();
          if (!active() || request.signal.aborted) return;
          apply(result);
          errors.delete(key);
        } catch (reason) {
          if (!active() || request.signal.aborted) return;
          errors.set(key, `${key === 1 ? 'Changes' : 'Commits'}：${reason instanceof Error ? reason.message : String(reason)}`);
        }
      };
      await Promise.all([
        mask & 1 ? run(1, () => gitApi.gitStatus(repository, request.signal), (value) => latest.current.callbacks.status(value)) : undefined,
        mask & 2 ? run(2, () => gitApi.gitLog(repository, request.signal), (value) => latest.current.callbacks.log(value)) : undefined,
      ]);
      controller = undefined;
      if (!active()) return;
      if (!request.signal.aborted) latest.current.callbacks.error([...errors.values()].join('；'));
      if (pending) {
        const next = pending; pending = 0;
        void refresh(next);
      } else {
        timer = window.setTimeout(() => void refresh(3), 5_000);
      }
    };
    const onVisibilityChange = () => {
      clearTimer();
      if (document.visibilityState === 'visible') void refresh(3);
      else { pending = 0; controller?.abort(); }
    };
    refreshRef.current = (mask) => void refresh(mask);
    document.addEventListener('visibilitychange', onVisibilityChange);
    void refresh(3);
    return () => {
      disposed = true;
      refreshRef.current = () => {};
      clearTimer();
      controller?.abort();
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [repository, visible]);

  useEffect(() => {
    if (previousGeneration.current !== generation) refreshRef.current(3);
    previousGeneration.current = generation;
  }, [generation]);

  return useCallback((target: RefreshTarget = 'all') => refreshRef.current(target === 'changes' ? 1 : target === 'commits' ? 2 : 3), []);
}
