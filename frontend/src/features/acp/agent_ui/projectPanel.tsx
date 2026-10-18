import { createContext, useContext, type ReactNode } from 'react';
import type { AowProject } from '../../../aow/types';
import { useInstancePanelPreferences } from '../../instances/useInstancePanelPreferences';

const ProjectPanelContext = createContext<(ReturnType<typeof useInstancePanelPreferences> & { project: AowProject }) | null>(null);
export function ProjectAcpPanelProvider({ project, children }: { project: AowProject; children: ReactNode }) {
  const preferences = useInstancePanelPreferences(project.id, 'acp');
  return <ProjectPanelContext.Provider value={{ ...preferences, project }}>{children}</ProjectPanelContext.Provider>;
}
export function useProjectAcpPanel() {
  const panel = useContext(ProjectPanelContext);
  if (!panel) throw new Error('Project ACP panel provider is missing');
  return panel;
}
