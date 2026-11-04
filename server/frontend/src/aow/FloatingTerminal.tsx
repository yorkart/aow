import { useEffect } from 'react';
import { TerminalWorkspace } from '../features/terminals/TerminalWorkspace';
import { useTerminals } from '../features/terminals/useTerminals';
import { useFloatingWorkspace, type FloatingTab } from './floatingWorkspaceState';
import { useAowTabLocation } from './AowTabEntry';

/** Terminals in temporary directories have no registered project surface. */
export function FloatingTerminal({ entry, activeLocation }: { entry: FloatingTab; activeLocation: boolean }) {
  const floating = useFloatingWorkspace();
  const syncLocation = useAowTabLocation();
  const terminals = useTerminals(entry.workspace, true);
  const active = floating.active?.workspace === entry.workspace && floating.active.id === entry.id;
  const tab = terminals.tabs.find(tab => tab.id === entry.targetId);
  useEffect(() => {
    if (active && activeLocation && tab) syncLocation(tab.id, '', floating.visible);
  }, [active, activeLocation, tab?.id, floating.visible, syncLocation]);
  useEffect(() => {
    if (terminals.loaded && !terminals.loading && !terminals.error && !tab) floating.remove(entry.workspace, entry.id);
  }, [terminals.loaded, terminals.loading, terminals.error, tab, floating.remove, entry.workspace, entry.id]);
  return <div className="floating-workspace-host" hidden={!active}>
    {terminals.error && <p role="alert">{terminals.error}</p>}
    <TerminalWorkspace visible={active && floating.visible} tab={tab} loading={terminals.loading}
      detectedAgents={terminals.detectedAgents} terminalTitles={terminals.terminalTitles} agentProcesses={terminals.agentProcesses} terminalActivity={terminals.terminalActivity}
      onTabChange={terminals.replace} onTabClosed={id => { terminals.remove(id); floating.remove(entry.workspace, entry.id); }}
      onPaneStatus={terminals.updatePaneStatus} onReload={() => void terminals.reload()} />
  </div>;
}
