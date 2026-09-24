import { useCallback, useEffect, useMemo, useState } from 'react';
import { readStored } from './floatingWorkspaceState';
import { terminalApi } from '../features/terminals/terminalApi';
import type { AowProject } from './types';
import { isWorktreeUnallocated, type WorktreeResourceState } from './worktreeResources';

interface StoredResources {
  documents?: unknown[];
  sessions?: unknown[];
  pullRequests?: unknown[];
  tasks?: unknown[];
}

// Inspect unmounted workspaces without restoring editors or attaching to terminals.
export function useWorktreeResources(projects: AowProject[], externalTabWorkspaces: ReadonlySet<string>) {
  const pathsKey = JSON.stringify(projects.flatMap(project => project.worktrees.map(worktree => worktree.path)).sort());
  const stored = useMemo(() => new Map((JSON.parse(pathsKey) as string[]).map(path => [
    path, readStored<StoredResources>(`aow-workspace-tabs:${path}`, {}),
  ])), [pathsKey]);
  const [terminalSnapshot, setTerminalSnapshot] = useState<{ pathsKey: string; paths: Set<string> }>();
  const [liveResources, setLiveResources] = useState<Map<string, WorktreeResourceState>>(() => new Map());
  const needsInventory = [...stored.keys()].some(path => !liveResources.has(path));

  useEffect(() => {
    if (!needsInventory) return;
    const controller = new AbortController();
    let pending = false;
    const refresh = async () => {
      if (pending || document.visibilityState !== 'visible') return;
      pending = true;
      try {
        const tabs = await terminalApi.list(undefined, controller.signal);
        if (controller.signal.aborted) return;
        const paths = new Set<string>();
        for (const tab of tabs) {
          if (stored.has(tab.workspace_root)) paths.add(tab.workspace_root);
        }
        setTerminalSnapshot(current => current?.pathsKey === pathsKey && current.paths.size === paths.size
          && [...paths].every(path => current.paths.has(path)) ? current : { pathsKey, paths });
      } catch {
        if (!controller.signal.aborted) setTerminalSnapshot(undefined);
      } finally {
        pending = false;
      }
    };
    void refresh();
    // Mounted workspaces already report their inventories through useTerminals.
    const timer = window.setInterval(() => void refresh(), 3000);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      controller.abort();
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', refresh);
    };
  }, [pathsKey, stored, needsInventory]);

  const reportResources = useCallback((path: string, resources?: WorktreeResourceState) => {
    setLiveResources(current => {
      const previous = current.get(path);
      if (previous === resources || (previous && resources
        && previous.hasFrontendTabs === resources.hasFrontendTabs
        && previous.hasTerminalInstances === resources.hasTerminalInstances)) return current;
      const next = new Map(current);
      if (resources === undefined) next.delete(path);
      else next.set(path, resources);
      return next;
    });
  }, []);

  const isUnallocated = useCallback((path: string) => {
    const saved = stored.get(path);
    const resources = liveResources.get(path) ?? {
      hasFrontendTabs: [saved?.documents, saved?.sessions, saved?.pullRequests, saved?.tasks]
        .some(items => Array.isArray(items) && items.length > 0),
      hasTerminalInstances: terminalSnapshot?.pathsKey === pathsKey ? terminalSnapshot.paths.has(path) : undefined,
    };
    return isWorktreeUnallocated({
      ...resources,
      hasFrontendTabs: externalTabWorkspaces.has(path) ? true : resources.hasFrontendTabs,
    });
  }, [liveResources, stored, terminalSnapshot, pathsKey, externalTabWorkspaces]);

  return { isUnallocated, reportResources };
}
