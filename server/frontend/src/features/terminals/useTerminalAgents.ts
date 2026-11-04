import { useMemo } from 'react';
import { useTerminalState } from './useTerminalState';
import { emptyTerminalMetadata } from './terminalStore';
import type { TerminalAgentList } from './types';

export function useTerminalAgents(workspaceRoot: string, enabled: boolean): TerminalAgentList {
  const snapshot = useTerminalState(enabled);
  return useMemo(() => {
    if (!enabled || !snapshot.tabs) return emptyTerminalMetadata;
    const panes = new Set(snapshot.tabs.filter(tab => tab.workspace_root === workspaceRoot).flatMap(tab => tab.panes.map(pane => pane.id)));
    const select = <T,>(values: Record<string, T>) => Object.fromEntries(Object.entries(values).filter(([id]) => panes.has(id)));
    return {
      agents: select(snapshot.metadata.agents), titles: select(snapshot.metadata.titles),
      processes: select(snapshot.metadata.processes), activity: select(snapshot.metadata.activity),
    };
  }, [enabled, workspaceRoot, snapshot.tabs, snapshot.metadata]);
}
