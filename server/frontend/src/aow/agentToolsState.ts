import { useCallback, useEffect, useState } from 'react';
import { persist, readStored } from './floatingWorkspaceState';

export type ToolView = 'properties' | 'conversations' | 'files' | 'git' | 'pullRequests' | 'automations' | 'terminal';
export interface ToolTab { id: string; view: ToolView; preview?: boolean }
export interface AgentToolsState { tabs: ToolTab[]; active?: string }
export function openTool(state: AgentToolsState, tab: ToolTab): AgentToolsState {
  if (state.tabs.some(item => item.id === tab.id)) return { ...state, active: tab.id };
  // A browsing entry becomes the first preview; kept editors are never replaced.
  const replace = state.tabs.findIndex(item => tab.preview && (item.preview || item.id === `view:${tab.view}`));
  const tabs = [...state.tabs];
  if (replace < 0) tabs.push(tab); else tabs.splice(replace, 1, tab);
  return { tabs, active: tab.id };
}
export function closeTool(state: AgentToolsState, id: string): AgentToolsState {
  const index = state.tabs.findIndex(tab => tab.id === id);
  const tabs = state.tabs.filter(tab => tab.id !== id);
  return { tabs, active: state.active === id ? tabs[Math.min(index, tabs.length - 1)]?.id : state.active };
}
export function useAgentTools(scope: string, owner: string, defaultView: ToolView) {
  const [owners, setOwners] = useState<Record<string, AgentToolsState>>(() => {
    const saved = readStored<Record<string, AgentToolsState>>(`aow-agent-tools:${scope}`, {});
    return Object.fromEntries(Object.entries(saved ?? {}).filter(([, state]) => state && Array.isArray(state.tabs)
      && state.tabs.every(tab => tab && typeof tab.id === 'string' && ['properties', 'conversations', 'files', 'git', 'pullRequests', 'automations', 'terminal'].includes(tab.view))));
  });
  const fallback: AgentToolsState = { tabs: [{ id: `view:${defaultView}`, view: defaultView }], active: `view:${defaultView}` };
  const current = owners[owner] ?? fallback;
  const update = useCallback((change: (state: AgentToolsState) => AgentToolsState) => {
    setOwners(previous => ({ ...previous, [owner]: change(previous[owner] ?? { tabs: [{ id: `view:${defaultView}`, view: defaultView }], active: `view:${defaultView}` }) }));
  }, [owner, defaultView]);
  useEffect(() => { persist(`aow-agent-tools:${scope}`, owners); }, [scope, owners]);
  const open = useCallback((tab: ToolTab) => update(state => openTool(state, tab)), [update]);
  const close = useCallback((id: string) => update(state => closeTool(state, id)), [update]);
  const pin = useCallback((id: string) => update(state => ({ ...state, tabs: state.tabs.map(tab => tab.id === id ? { ...tab, preview: false } : tab) })), [update]);
  const replaceResource = useCallback((id: string, replacement?: string) => {
    setOwners(previous => {
      if (!Object.values(previous).some(state => state.tabs.some(tab => tab.id === id))) return previous;
      return Object.fromEntries(Object.entries(previous).map(([key, state]) => [key, replacement
        ? { tabs: state.tabs.map(tab => tab.id === id ? { ...tab, id: replacement } : tab), active: state.active === id ? replacement : state.active }
        : closeTool(state, id)]));
    });
  }, []);
  const pinResource = useCallback((id: string) => {
    setOwners(previous => {
      if (!Object.values(previous).some(state => state.tabs.some(tab => tab.id === id && tab.preview))) return previous;
      return Object.fromEntries(Object.entries(previous).map(([key, state]) => [key, {
        ...state, tabs: state.tabs.map(tab => tab.id === id ? { ...tab, preview: false } : tab),
      }]));
    });
  }, []);
  return { ...current, owners, open, close, pin, replaceResource, pinResource };
}
