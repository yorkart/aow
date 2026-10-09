import type { AowAgent } from './types';
import { aowRequest } from '../../lib/aowRequest';

export const agentsApi = {
  agents: (refresh = false) => aowRequest<AowAgent[]>(`/api/aow/agents?refresh=${refresh}`),
  registerAgent: (agent: { id: string; agentType: NonNullable<AowAgent['agent_type']>; displayName: string; command: string; args: string[]; env: Record<string, string> }, existingId?: string) => aowRequest<AowAgent>(existingId ? `/api/aow/agents/${encodeURIComponent(existingId)}` : '/api/aow/agents', {
    method: existingId ? 'PUT' : 'POST',
    body: JSON.stringify({
      id: agent.id,
      agent_type: agent.agentType,
      display_name: agent.displayName,
      command: agent.command,
      args: agent.args,
      env: agent.env,
    }),
  }),
  removeAgent: (id: string) => aowRequest<void>(`/api/aow/agents/${encodeURIComponent(id)}`, { method: 'DELETE' }),
};
