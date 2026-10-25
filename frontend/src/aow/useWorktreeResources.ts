import { useCallback, useMemo, useState } from 'react';
import { readStored } from './floatingWorkspaceState';
import { useTerminalState } from '../features/terminals/useTerminalState';
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
  const [liveResources, setLiveResources] = useState<Map<string, WorktreeResourceState>>(() => new Map());
  const needsInventory = [...stored.keys()].some(path => !liveResources.has(path));

  const terminalSnapshot = useTerminalState(needsInventory);
  const terminalPaths = useMemo(() => terminalSnapshot.tabs && new Set(terminalSnapshot.tabs.map(tab => tab.workspace_root)), [terminalSnapshot.tabs]);

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
      hasTerminalInstances: terminalSnapshot.error ? undefined : terminalPaths?.has(path),
    };
    return isWorktreeUnallocated({
      ...resources,
      hasFrontendTabs: externalTabWorkspaces.has(path) ? true : resources.hasFrontendTabs,
    });
  }, [liveResources, stored, terminalPaths, terminalSnapshot.error, externalTabWorkspaces]);

  return { isUnallocated, reportResources };
}
