import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react';
import type { AowProject } from '../../aow/types';
import { useInstancePanelPreferences } from '../instances/useInstancePanelPreferences';
import { useProjectTerminals } from './useProjectTerminals';

type ProjectTerminalPanelState = ReturnType<typeof useInstancePanelPreferences> & {
  inventory: ReturnType<typeof useProjectTerminals>;
  subscribe: () => () => void;
};

const ProjectTerminalPanelContext = createContext<ProjectTerminalPanelState | null>(null);

// Keep one inventory and one set of preferences while switching between a project's worktrees.
export function ProjectTerminalPanelProvider({ project, children }: { project: AowProject; children: ReactNode }) {
  const preferences = useInstancePanelPreferences(project.id, 'terminal');
  const [subscribers, setSubscribers] = useState(0);
  const subscribe = useCallback(() => {
    setSubscribers(count => count + 1);
    return () => setSubscribers(count => count - 1);
  }, []);
  const inventory = useProjectTerminals(project.worktrees, !project.builtin && subscribers > 0);
  return <ProjectTerminalPanelContext.Provider value={{ inventory, subscribe, ...preferences }}>{children}</ProjectTerminalPanelContext.Provider>;
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
