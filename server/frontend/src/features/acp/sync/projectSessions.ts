import { useMemo, useSyncExternalStore } from 'react';
import type { SessionInfo } from '../types';
import { getResource, type SyncState } from './store';

// Aggregate metadata subscriptions only. ResourceStore shares each cwd subscription
// with other views and releases it when the last consumer leaves.
export function useProjectSessions(paths: string[], visible: boolean) {
  const key = JSON.stringify(visible ? [...new Set(paths)].sort() : []);
  const collection = useMemo(() => {
    const stores = (JSON.parse(key) as string[]).map(cwd => ({ cwd, store: getResource({ kind: 'sessions', cwd }) }));
    let previous: SyncState[] = [];
    let snapshot = { data: [] as SessionInfo[], workspaces: [] as { cwd: string; loading: boolean; error: string }[] };
    return {
      subscribe: (listener: () => void) => {
        const unsubscribe = stores.map(({ store }) => store.subscribe(listener));
        return () => unsubscribe.forEach(dispose => dispose());
      },
      snapshot: () => {
        const states = stores.map(({ store }) => store.snapshot());
        if (states.length !== previous.length || states.some((state, index) => state !== previous[index])) {
          previous = states;
          snapshot = {
            data: states.flatMap((state, index) => ((state.data ?? []) as SessionInfo[]).filter(session => session.cwd === stores[index].cwd)),
            workspaces: states.map((state, index) => ({ cwd: stores[index].cwd, loading: state.loading && !state.data, error: state.error })),
          };
        }
        return snapshot;
      },
      reload: () => stores.forEach(({ store }) => store.refresh()),
    };
  }, [key]);
  const snapshot = useSyncExternalStore(collection.subscribe, collection.snapshot);
  return {
    data: snapshot.data,
    statusFor: (workspace: string, allWorktrees: boolean) => {
      const states = snapshot.workspaces.filter(state => allWorktrees || state.cwd === workspace);
      return {
        loading: states.some(state => state.loading),
        error: states.map(state => state.error ? `${state.cwd}: ${state.error}` : '').filter(Boolean).join('\n'),
      };
    },
    reload: collection.reload,
  };
}
