import { useEffect, useState } from 'react';
import { terminalApi } from './terminalApi';
import type { TerminalAgentList } from './types';

const emptyMetadata: TerminalAgentList = { agents: {}, titles: {}, processes: {} };

function sameValues(a: Record<string, string | null>, b: Record<string, string | null>) {
  return Object.keys(a).length === Object.keys(b).length
    && Object.entries(a).every(([id, value]) => b[id] === value);
}

export function useTerminalAgents(workspaceRoot: string, enabled: boolean) {
  const [snapshot, setSnapshot] = useState({ root: workspaceRoot, metadata: emptyMetadata });

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    let pending = false;
    const update = (metadata: TerminalAgentList) => {
      if (controller.signal.aborted) return;
      setSnapshot(previous => previous.root === workspaceRoot
        && sameValues(previous.metadata.agents, metadata.agents)
        && sameValues(previous.metadata.titles, metadata.titles)
        && Object.keys(previous.metadata.processes).length === Object.keys(metadata.processes).length
        && Object.entries(previous.metadata.processes).every(([id, process]) => {
          const next = metadata.processes[id];
          return next?.pid === process.pid && next.start_time === process.start_time && next.cwd === process.cwd;
        })
        ? previous : { root: workspaceRoot, metadata });
    };
    const refresh = async () => {
      if (pending || document.visibilityState !== 'visible') return;
      pending = true;
      try { update(await terminalApi.agents(workspaceRoot, controller.signal)); }
      catch { /* A failed poll is not evidence that the agent exited. */ }
      finally { pending = false; }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1500);
    document.addEventListener('visibilitychange', refresh);
    return () => {
      controller.abort();
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', refresh);
    };
  }, [workspaceRoot, enabled]);

  return snapshot.root === workspaceRoot ? snapshot.metadata : emptyMetadata;
}
