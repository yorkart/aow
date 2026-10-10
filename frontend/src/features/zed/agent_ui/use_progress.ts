import { useEffect, useState } from 'react';
import { acpApi } from '../api';
import type { ConnectionStatus } from '../types';

export function useProgress(agentId: string, workspace?: string) {
  const [status, setStatus] = useState<ConnectionStatus>();
  useEffect(() => {
    setStatus(undefined);
    if (!agentId) return;
    const controller = new AbortController(); let timer: number;
    const poll = async () => {
      try {
        const next = workspace === undefined ? await acpApi.installationStatus(agentId, controller.signal) : await acpApi.connectionStatus(agentId, workspace, controller.signal);
        if (!controller.signal.aborted && next?.running) setStatus(next);
      } catch { /* The operation reports its own failure. */ }
      if (!controller.signal.aborted) timer = window.setTimeout(() => { void poll(); }, 750);
    };
    void poll();
    return () => { controller.abort(); window.clearTimeout(timer); };
  }, [agentId, workspace]);
  return status;
}
