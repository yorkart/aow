import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { terminalApi } from './terminalApi';
import { isCliTerminal } from './terminalPresentation';
import { useTerminalAgents } from './useTerminalAgents';
import type { TerminalLayout, TerminalPaneStatus, TerminalTab } from './types';

function message(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

function reorderTabs(items: TerminalTab[], tabIds: string[]) {
  if (items.length !== tabIds.length) return items;
  const byId = new Map(items.map((tab) => [tab.id, tab]));
  const ordered = tabIds.map((id) => byId.get(id));
  return ordered.every((tab): tab is TerminalTab => tab !== undefined) ? ordered : items;
}

function sameLayout(left: TerminalLayout | null, right: TerminalLayout | null): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  if (left.type === 'pane') return right.type === 'pane' && left.pane_id === right.pane_id;
  return right.type === 'split' && left.axis === right.axis && left.ratio === right.ratio
    && sameLayout(left.first, right.first) && sameLayout(left.second, right.second);
}

export function useTerminals(workspaceRoot: string, enabled: boolean) {
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  const [activeId, setActiveId] = useState<string>();
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const busyRef = useRef(busy);
  busyRef.current = busy;
  const requestGeneration = useRef(0);
  const busyGeneration = useRef(0);
  const workspaceGeneration = useRef(0);
  const workspaceRootRef = useRef(workspaceRoot);
  const stateRootRef = useRef(workspaceRoot);
  const loadedRootRef = useRef<string | undefined>(undefined);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;

  if (workspaceRootRef.current !== workspaceRoot) {
    workspaceRootRef.current = workspaceRoot;
    workspaceGeneration.current += 1;
  }

  const load = useCallback(async (quiet = false) => {
    if (workspaceRootRef.current !== workspaceRoot || (quiet && busyRef.current)) return;
    const generation = ++requestGeneration.current;
    const rootGeneration = workspaceGeneration.current;
    loadedRootRef.current = workspaceRoot;
    if (!quiet) { setLoading(true); setError(''); }
    try {
      const nextTabs = await terminalApi.list(workspaceRoot);
      if (requestGeneration.current !== generation
        || workspaceGeneration.current !== rootGeneration
        || workspaceRootRef.current !== workspaceRoot) return;
      stateRootRef.current = workspaceRoot;
      setTabs(current => {
        const previousById = new Map(current.map(tab => [tab.id, tab]));
        return nextTabs.map(tab => {
          const previous = previousById.get(tab.id);
          // A fresh JSON object must not reset TerminalWorkspace's active drag.
          return previous && sameLayout(previous.layout, tab.layout)
            ? { ...tab, layout: previous.layout }
            : tab;
        });
      });
      setLoaded(true);
      setError('');
      setActiveId((current) => nextTabs.some((tab) => tab.id === current) ? current : nextTabs.find(tab => !isCliTerminal(tab))?.id);
    } catch (reason) {
      if (requestGeneration.current === generation
        && workspaceGeneration.current === rootGeneration
        && workspaceRootRef.current === workspaceRoot) setError(message(reason));
    } finally {
      if (requestGeneration.current === generation
        && workspaceGeneration.current === rootGeneration
        && workspaceRootRef.current === workspaceRoot) setLoading(false);
    }
  }, [workspaceRoot]);

  useEffect(() => {
    requestGeneration.current += 1;
    busyGeneration.current += 1;
    loadedRootRef.current = undefined;
    stateRootRef.current = workspaceRoot;
    setTabs([]);
    setLoaded(false);
    setActiveId(undefined);
    setLoading(false);
    setBusy(false);
    setError('');
  }, [workspaceRoot]);

  useEffect(() => {
    if (enabled && loadedRootRef.current !== workspaceRoot) void load();
  }, [enabled, load]);

  useEffect(() => {
    if (!enabled) return;
    let pending = false;
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible' && !pending) {
        pending = true;
        void load(true).finally(() => { pending = false; });
      }
    }, 3000);
    return () => window.clearInterval(timer);
  }, [enabled, load]);

  const create = useCallback(async (agentId?: string) => {
    if (workspaceRootRef.current !== workspaceRoot) return null;
    requestGeneration.current += 1;
    const operationGeneration = ++busyGeneration.current;
    const rootGeneration = workspaceGeneration.current;
    setLoading(false);
    setBusy(true);
    setError('');
    try {
      const tab = await terminalApi.create(workspaceRoot, { cwd: workspaceRoot, agentId });
      if (workspaceGeneration.current !== rootGeneration
        || workspaceRootRef.current !== workspaceRoot) return null;
      stateRootRef.current = workspaceRoot;
      setTabs((items) => [...items.filter((item) => item.id !== tab.id), tab]);
      setActiveId(tab.id);
      return tab;
    } catch (reason) {
      if (workspaceGeneration.current === rootGeneration
        && workspaceRootRef.current === workspaceRoot) setError(message(reason));
      return null;
    } finally {
      if (busyGeneration.current === operationGeneration
        && workspaceGeneration.current === rootGeneration
        && workspaceRootRef.current === workspaceRoot) setBusy(false);
    }
  }, [workspaceRoot]);

  const rename = useCallback(async (tabId: string, nextName: string) => {
    const name = nextName.trim();
    if (!name) return;
    const root = workspaceRootRef.current;
    const rootGeneration = workspaceGeneration.current;
    const operationGeneration = ++busyGeneration.current;
    requestGeneration.current += 1;
    setLoading(false);
    setBusy(true);
    setError('');
    try {
      const updated = await terminalApi.rename(tabId, name);
      if (workspaceGeneration.current !== rootGeneration || workspaceRootRef.current !== root) return;
      setTabs((items) => items.map((tab) => tab.id === tabId ? { ...(updated ?? tab), name, name_is_custom: true } : tab));
    } catch (reason) {
      if (workspaceGeneration.current === rootGeneration && workspaceRootRef.current === root) setError(message(reason));
    } finally {
      if (busyGeneration.current === operationGeneration
        && workspaceGeneration.current === rootGeneration
        && workspaceRootRef.current === root) setBusy(false);
    }
  }, []);

  const reorder = useCallback(async (tabIds: string[]) => {
    const root = workspaceRootRef.current;
    const rootGeneration = workspaceGeneration.current;
    const previousIds = tabsRef.current.map((tab) => tab.id);
    if (previousIds.length !== tabIds.length
      || previousIds.some((id) => !tabIds.includes(id))
      || previousIds.every((id, index) => id === tabIds[index])) return;
    const operationGeneration = ++busyGeneration.current;
    requestGeneration.current += 1;
    setLoading(false);
    setBusy(true);
    setError('');
    setTabs((items) => reorderTabs(items, tabIds));
    try {
      const reordered = await terminalApi.reorder(root, tabIds);
      if (workspaceGeneration.current !== rootGeneration || workspaceRootRef.current !== root) return;
      stateRootRef.current = root;
      setTabs((items) => reorderTabs(items, reordered.map((tab) => tab.id)));
    } catch (reason) {
      if (workspaceGeneration.current === rootGeneration && workspaceRootRef.current === root) {
        setTabs((items) => reorderTabs(items, previousIds));
        setError(message(reason));
      }
    } finally {
      if (busyGeneration.current === operationGeneration
        && workspaceGeneration.current === rootGeneration
        && workspaceRootRef.current === root) setBusy(false);
    }
  }, []);

  const close = useCallback(async (tabId: string) => {
    const root = workspaceRootRef.current;
    const rootGeneration = workspaceGeneration.current;
    const operationGeneration = ++busyGeneration.current;
    requestGeneration.current += 1;
    setLoading(false);
    setBusy(true);
    setError('');
    try {
      await terminalApi.close(tabId);
      if (workspaceGeneration.current !== rootGeneration || workspaceRootRef.current !== root) return;
      setTabs((items) => {
        const index = items.findIndex((item) => item.id === tabId);
        const remaining = items.filter((item) => item.id !== tabId);
        setActiveId((current) => current === tabId
          ? remaining[Math.min(Math.max(index, 0), remaining.length - 1)]?.id
          : current);
        return remaining;
      });
    } catch (reason) {
      if (workspaceGeneration.current === rootGeneration && workspaceRootRef.current === root) setError(message(reason));
    } finally {
      if (busyGeneration.current === operationGeneration
        && workspaceGeneration.current === rootGeneration
        && workspaceRootRef.current === root) setBusy(false);
    }
  }, []);

  const remove = useCallback((tabId: string) => {
    setTabs((items) => {
      const index = items.findIndex((item) => item.id === tabId);
      if (index < 0) return items;
      const remaining = items.filter((item) => item.id !== tabId);
      setActiveId((current) => current === tabId
        ? remaining[Math.min(index, remaining.length - 1)]?.id
        : current);
      return remaining;
    });
  }, []);

  const replace = useCallback((updated: TerminalTab) => {
    if (updated.workspace_root !== workspaceRootRef.current) return;
    requestGeneration.current += 1;
    setLoading(false);
    setTabs((items) => items.some((tab) => tab.id === updated.id)
      ? items.map((tab) => tab.id === updated.id
        ? typeof tab.revision === 'number' && typeof updated.revision === 'number' && updated.revision < tab.revision
          ? tab
          : updated
        : tab)
      : [...items, updated]);
  }, []);

  const updatePaneStatus = useCallback((tabId: string, paneId: string, status: TerminalPaneStatus, exitCode?: number | null) => {
    const currentRoot = workspaceRootRef.current;
    if (stateRootRef.current !== currentRoot
      || !tabsRef.current.some((tab) => tab.id === tabId && tab.workspace_root === currentRoot)) return;
    requestGeneration.current += 1;
    setLoading(false);
    setTabs((items) => items.map((tab) => tab.id === tabId ? {
      ...tab,
      panes: tab.panes.map((pane) => pane.id === paneId
        ? { ...pane, status, ...(exitCode === undefined ? {} : { exit_code: exitCode }) }
        : pane),
    } : tab));
  }, []);

  const currentTabs = stateRootRef.current === workspaceRoot ? tabs : [];
  const { agents: detectedAgents, titles: terminalTitles, processes: agentProcesses } = useTerminalAgents(workspaceRoot, enabled && currentTabs.length > 0);
  const currentActiveId = stateRootRef.current === workspaceRoot ? activeId : undefined;
  const activeTab = useMemo(
    () => currentTabs.find((tab) => tab.id === currentActiveId),
    [currentActiveId, currentTabs],
  );

  return {
    tabs: currentTabs, activeId: currentActiveId, activeTab, loaded, loading, busy, error, detectedAgents, terminalTitles, agentProcesses,
    activate: setActiveId, create, rename, reorder, close, remove, replace, updatePaneStatus, reload: load,
  };
}
