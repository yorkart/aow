import { appLocalStorage } from '../lib/basePath';
import { useCallback, useEffect, useRef, useState, type SetStateAction } from 'react';
import { aowApi } from './aowApi';
import type { PinnedWorktrees } from './types';

const legacyStorageKey = 'aow-pinned-worktrees';
const migratedStorageKey = 'aow-pinned-worktrees-migrated';

function legacyPins() {
  try {
    if (appLocalStorage.getItem(migratedStorageKey) === '1') return [];
    const value: unknown = JSON.parse(appLocalStorage.getItem(legacyStorageKey) ?? '[]');
    return Array.isArray(value) ? value.filter((path): path is string => typeof path === 'string' && path.startsWith('/') && !path.includes('\0')) : [];
  } catch { return []; }
}

export function usePinnedWorktrees() {
  const [pinned, setPinned] = useState(new Set<string>());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const current = useRef(new Set<string>());
  const pending = useRef(0);
  const foregroundPending = useRef(0);
  const queue = useRef(Promise.resolve());
  const accept = useCallback((result: PinnedWorktrees) => {
    const next = new Set(result.paths);
    const paths = [...next];
    if (current.current.size === next.size
      && [...current.current].every((path, index) => path === paths[index])) return;
    current.current = next;
    setPinned(next);
  }, []);
  const run = useCallback((task: () => Promise<void>, background = false) => {
    pending.current += 1;
    if (!background) {
      foregroundPending.current += 1;
      setLoading(true);
    }
    queue.current = queue.current.then(async () => {
      await task();
      setError('');
    }).catch((reason: unknown) => {
      setError(`置顶同步失败：${reason instanceof Error ? reason.message : String(reason)}`);
    }).finally(() => {
      pending.current -= 1;
      if (!background) {
        foregroundPending.current -= 1;
        setLoading(foregroundPending.current > 0);
      }
    });
  }, []);
  const load = useCallback((background = false) => {
    if (pending.current > 0) return;
    run(async () => {
      accept(await aowApi.pinnedWorktrees());
      const legacy = legacyPins();
      if (legacy.length) {
        accept(await aowApi.updatePinnedWorktrees({ add: legacy }));
        try {
          appLocalStorage.setItem(migratedStorageKey, '1');
          appLocalStorage.removeItem(legacyStorageKey);
        } catch { /* Server persistence succeeded even if browser cleanup is unavailable. */ }
      }
    }, background);
  }, [accept, run]);
  const reload = useCallback(() => load(), [load]);
  useEffect(() => {
    load();
    const refresh = () => { if (document.visibilityState === 'visible') load(true); };
    const timer = window.setInterval(refresh, 10000);
    document.addEventListener('visibilitychange', refresh);
    window.addEventListener('focus', refresh);
    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', refresh);
      window.removeEventListener('focus', refresh);
    };
  }, [load]);
  const update = useCallback((action: SetStateAction<Set<string>>) => {
    run(async () => {
      const before = current.current;
      const next = typeof action === 'function' ? action(new Set(before)) : action;
      const add = [...next].filter((path) => !before.has(path));
      const remove = [...before].filter((path) => !next.has(path));
      const order = !add.length && !remove.length ? [...next] : undefined;
      accept(await aowApi.updatePinnedWorktrees({ add, remove, order }));
    });
  }, [accept, run]);
  return [pinned, update, { loading, error, reload }] as const;
}
