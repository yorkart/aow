import { useOperationSync } from '../sync/hooks';

export function useProgress(agentId: string, workspace?: string) {
  return useOperationSync(agentId, workspace).data ?? undefined;
}
