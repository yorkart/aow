// View-owned drafts outlive DOM/portal mounts, but are discarded when a tab closes.
import { useMemo, useSyncExternalStore } from 'react';
import { emptyDraft, type ComposerDraft, type UpdateDraft } from './message_editor/model';

function createDraft() {
  let value: ComposerDraft = emptyDraft();
  const listeners = new Set<() => void>();
  return {
    read: () => value,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    write: ((next) => {
      const updated = typeof next === 'function' ? next(value) : next;
      if (updated === value) return;
      value = updated;
      for (const listener of listeners) listener();
    }) satisfies UpdateDraft,
  };
}
const drafts = new Map<string, ReturnType<typeof createDraft>>();
const key = (workspace: string, tabId: string) => JSON.stringify([workspace, tabId]);

export function discardComposerDraft(workspace: string, tabId: string) {
  drafts.delete(key(workspace, tabId));
}

export function discardWorkspaceComposerDrafts(workspace: string) {
  for (const id of drafts.keys()) if (JSON.parse(id)[0] === workspace) drafts.delete(id);
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
