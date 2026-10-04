import { useCallback, useEffect, useRef, useState } from 'react';
import { subscribeWorkspaceChanges } from '../../lib/workspaceEvents';
import { inboxApi } from './api';
import { inboxError, type InboxSnapshot } from './types';

export function useInbox(visible: boolean) {
  const [data, setData] = useState<InboxSnapshot>();
  const [error, setError] = useState('');
  const generation = useRef(0);
  const reload = useCallback(async () => {
    const ticket = ++generation.current;
    try {
      const next = await inboxApi.list();
      if (ticket === generation.current) { setData(next); setError(''); }
    } catch (reason) { if (ticket === generation.current) setError(inboxError(reason)); }
  }, []);
  useEffect(() => {
    if (!visible) return;
    void reload();
    return subscribeWorkspaceChanges(change => { if (change.reset || change.inbox) void reload(); });
  }, [visible, reload]);
  const starting = data?.executions.some(run => run.phase === 'starting');
  useEffect(() => {
    if (!visible || !starting) return;
    const timer = window.setInterval(() => { if (document.visibilityState === 'visible') void reload(); }, 1500);
    return () => window.clearInterval(timer);
  }, [visible, starting, reload]);
  useEffect(() => () => { generation.current++; }, []);
  const mutate = useCallback(async <T,>(action: () => Promise<T>): Promise<T> => {
    try { return await action(); } finally { await reload(); }
  }, [reload]);
  return { data, error, reload, mutate };
}
