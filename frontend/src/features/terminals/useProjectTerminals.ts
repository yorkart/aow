import { useCallback, useEffect, useMemo, useState } from 'react';
import { terminalApi } from './terminalApi';
import type { TerminalAgentList, TerminalPaneStatus, TerminalTab } from './types';
import type { AowWorktree } from '../../aow/types';
import { subscribeWorkspaceChanges } from '../../lib/workspaceEvents';

const empty = { tabs: [] as TerminalTab[], agents: {}, titles: {}, processes: {}, activity: {} };

// Listing a repository's terminals must not mount their emulators or attach to PTYs.
export function useProjectTerminals(worktrees: AowWorktree[], enabled: boolean) {
  const rootsKey = JSON.stringify(worktrees.map(worktree => worktree.path).sort());
  const roots = useMemo(() => new Set<string>(JSON.parse(rootsKey)), [rootsKey]);
  const [snapshot, setSnapshot] = useState<(TerminalAgentList & { rootsKey: string; tabs: TerminalTab[] })>();
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const reload = useCallback(() => setRevision(value => value + 1), []);

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    let pending = false;
    let queued = false;
    const refresh = async () => {
      if (pending) { queued = true; return; }
      if (document.visibilityState !== 'visible' || controller.signal.aborted) return;
      pending = true;
      try {
        const [tabs, metadata] = await Promise.all([
          terminalApi.list(undefined, controller.signal),
          terminalApi.agents(undefined, controller.signal),
        ]);
        if (controller.signal.aborted) return;
        setSnapshot({ rootsKey, tabs: tabs.filter(tab => roots.has(tab.workspace_root)), ...metadata });
        setError('');
      } catch (reason) {
        if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        pending = false;
        if (queued && !controller.signal.aborted) { queued = false; void refresh(); }
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3000);
    const unsubscribe = subscribeWorkspaceChanges(change => { if (change.reset || change.terminals) void refresh(); });
    document.addEventListener('visibilitychange', refresh);
    return () => {
      controller.abort();
      unsubscribe();
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', refresh);
    };
  }, [enabled, rootsKey, roots, revision]);

  const remove = useCallback((id: string) => {
    setSnapshot(current => current && { ...current, tabs: current.tabs.filter(tab => tab.id !== id) });
    reload();
  }, [reload]);

  const updatePaneStatus = useCallback((tabId: string, paneId: string, status: TerminalPaneStatus, exitCode?: number | null) => {
    setSnapshot(current => current && { ...current, tabs: current.tabs.map(tab => tab.id === tabId ? {
      ...tab, panes: tab.panes.map(pane => pane.id === paneId
        ? { ...pane, status, ...(exitCode === undefined ? {} : { exit_code: exitCode }) } : pane),
    } : tab) });
  }, []);

  const current = snapshot?.rootsKey === rootsKey ? snapshot : undefined;
  return { ...(current ?? empty), error, loaded: !!current, loading: enabled && !current && !error, reload, remove, updatePaneStatus };
}
