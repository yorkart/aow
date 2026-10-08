import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react';
import type { AowProject } from '../../aow/types';
import { persist, readStored } from '../../aow/floatingWorkspaceState';
import type { TerminalGroup } from './TerminalPanel';
import type { TerminalSort } from './TerminalScopeMenu';
import { useProjectTerminals } from './useProjectTerminals';

type Scopes = Record<TerminalGroup, boolean>;
type Sorts = Record<TerminalGroup, TerminalSort>;
type ProjectTerminalPanelState = {
  inventory: ReturnType<typeof useProjectTerminals>;
  scopes: Scopes;
  sorts: Sorts;
  setScope: (group: TerminalGroup, value: boolean) => void;
  setSort: (group: TerminalGroup, value: TerminalSort) => void;
  subscribe: () => () => void;
};

const ProjectTerminalPanelContext = createContext<ProjectTerminalPanelState | null>(null);

// Keep one inventory and one set of preferences while switching between a project's worktrees.
export function ProjectTerminalPanelProvider({ project, children }: { project: AowProject; children: ReactNode }) {
  const scopeKey = `aow-project-terminal-scope:${project.id}`;
  const sortKey = `aow-project-terminal-sort:${project.id}`;
  const [scopes, setScopes] = useState<Scopes>(() => {
    const stored = readStored<Partial<Scopes>>(scopeKey, {});
    return { user: stored.user !== false, cli: stored.cli !== false };
  });
  const [sorts, setSorts] = useState<Sorts>(() => {
    const stored = readStored<Partial<Sorts>>(sortKey, {});
    return { user: stored.user === 'createdAt' ? 'createdAt' : 'branch', cli: stored.cli === 'createdAt' ? 'createdAt' : 'branch' };
  });
  const [subscribers, setSubscribers] = useState(0);
  const subscribe = useCallback(() => {
    setSubscribers(count => count + 1);
    return () => setSubscribers(count => count - 1);
  }, []);
  const inventory = useProjectTerminals(project.worktrees, !project.builtin && subscribers > 0 && (scopes.user || scopes.cli));
  useEffect(() => persist(scopeKey, scopes), [scopeKey, scopes]);
  useEffect(() => persist(sortKey, sorts), [sortKey, sorts]);
  return <ProjectTerminalPanelContext.Provider value={{ inventory, scopes, sorts, subscribe,
    setScope: (group, value) => setScopes(current => ({ ...current, [group]: value })),
    setSort: (group, value) => setSorts(current => ({ ...current, [group]: value })),
  }}>{children}</ProjectTerminalPanelContext.Provider>;
}

export function useProjectTerminalPanel(visible: boolean) {
  const panel = useContext(ProjectTerminalPanelContext);
  if (!panel) throw new Error('Project terminal panel provider is missing');
  const { subscribe } = panel;
  useEffect(() => {
    if (visible) return subscribe();
  }, [subscribe, visible]);
  return panel;
}
