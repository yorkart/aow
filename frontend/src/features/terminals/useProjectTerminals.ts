import { useCallback, useMemo } from 'react';
import type { AowWorktree } from '../../aow/types';
import type { TerminalPaneStatus } from './types';
import { useTerminalState } from './useTerminalState';
import { terminalStore } from './terminalStore';

// Project inventories and mounted workspaces share one state subscription.
export function useProjectTerminals(worktrees: AowWorktree[], enabled: boolean) {
  const snapshot = useTerminalState(enabled);
  const rootsKey = JSON.stringify(worktrees.map(worktree => worktree.path).sort());
  const tabs = useMemo(() => {
    const roots = new Set<string>(JSON.parse(rootsKey));
    return snapshot.tabs?.filter(tab => roots.has(tab.workspace_root)) ?? [];
  }, [rootsKey, snapshot.tabs]);
  const remove = useCallback((id: string) => terminalStore.remove(id), []);
  const updatePaneStatus = useCallback((tabId: string, paneId: string, status: TerminalPaneStatus, exitCode?: number | null) => {
    terminalStore.updatePaneStatus(tabId, paneId, status, exitCode);
  }, []);
  return { tabs, ...snapshot.metadata, error: snapshot.error, loaded: snapshot.tabs !== undefined,
    loading: enabled && !snapshot.tabs && !snapshot.error, reload: terminalStore.refresh, remove, updatePaneStatus };
}
