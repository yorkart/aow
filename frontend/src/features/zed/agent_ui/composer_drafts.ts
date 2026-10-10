// View-owned drafts outlive DOM/portal mounts, but are discarded when a tab closes.
import { useMemo, useSyncExternalStore } from 'react';

function createDraft() {
  let value = '';
  const listeners = new Set<() => void>();
  return {
    read: () => value,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    write: (next: string | ((current: string) => string)) => {
      const updated = typeof next === 'function' ? next(value) : next;
      if (updated === value) return;
      value = updated;
      for (const listener of listeners) listener();
    },
  };
}
const drafts = new Map<string, ReturnType<typeof createDraft>>();
const key = (workspace: string, tabId: string) => JSON.stringify([workspace, tabId]);

export function discardComposerDraft(workspace: string, tabId: string) {
  drafts.delete(key(workspace, tabId));
}

export function useComposerDraft(workspace: string, tabId: string) {
  const draft = useMemo(() => {
    const id = key(workspace, tabId);
    let value = drafts.get(id);
    if (!value) { value = createDraft(); drafts.set(id, value); }
    return value;
  }, [workspace, tabId]);
  return [useSyncExternalStore(draft.subscribe, draft.read), draft.write] as const;
}
