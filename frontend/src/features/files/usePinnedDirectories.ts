import { useCallback, useEffect, useRef, useState } from 'react';
import { filesApi } from './api';
import type { PinnedDirectories } from './types';

export function usePinnedDirectories() {
  const [paths, setPaths] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [pending, setPending] = useState<Set<string>>(() => new Set());
  const pendingRef = useRef(new Set<string>());
  const revisionRef = useRef(-1);
  const reloadRef = useRef(0);

  const accept = useCallback((result: PinnedDirectories) => {
    if (result.revision < revisionRef.current) return;
    revisionRef.current = result.revision;
    setPaths(result.paths);
  }, []);

  const reload = useCallback(async () => {
    const generation = ++reloadRef.current;
    setLoading(true);
    setError('');
    try {
      const result = await filesApi.pinnedDirectories();
      if (generation === reloadRef.current) accept(result);
    } catch (reason) {
      if (generation === reloadRef.current) setError(`收藏加载失败：${reason instanceof Error ? reason.message : String(reason)}`);
    } finally {
      if (generation === reloadRef.current) setLoading(false);
    }
  }, [accept]);

  useEffect(() => { void reload(); }, [reload]);

  const toggle = useCallback(async (path: string, pinned: boolean) => {
    if (pendingRef.current.has(path)) return;
    pendingRef.current.add(path);
    setPending(new Set(pendingRef.current));
    setError('');
    try {
      accept(await filesApi.updatePinnedDirectories(pinned ? { remove: [path] } : { add: [path] }));
    } catch (reason) {
      setError(`收藏保存失败：${reason instanceof Error ? reason.message : String(reason)}`);
    } finally {
      pendingRef.current.delete(path);
      setPending(new Set(pendingRef.current));
    }
  }, [accept]);

  return { paths, loading, error, pending, reload, toggle };
}
