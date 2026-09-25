import type { AowAgent, AowAgentType } from './types';

export const agentTypes: { id: AowAgentType; label: string }[] = [
  { id: 'claude', label: 'Claude Code' },
  { id: 'codex', label: 'Codex' },
  { id: 'traecli', label: 'TraeCode CLI' },
  { id: 'hermes', label: 'Hermes' },
];

export function builtinAgentType(id?: string): AowAgentType | undefined {
  return agentTypes.find(type => type.id === id)?.id;
}

export function aowAgentType(agent: AowAgent): AowAgentType | undefined {
  return agent.agent_type ?? builtinAgentType(agent.id);
}
