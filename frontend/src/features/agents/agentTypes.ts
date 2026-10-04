import type { AowAgent, AowAgentType } from './types';

export const agentTypes: { id: AowAgentType; label: string; sessionTitleSource: 'terminal' | 'native' }[] = [
  { id: 'claude', label: 'Claude Code', sessionTitleSource: 'terminal' },
  { id: 'codex', label: 'Codex', sessionTitleSource: 'terminal' },
  { id: 'traecli', label: 'TraeCode CLI', sessionTitleSource: 'terminal' },
  { id: 'hermes', label: 'Hermes', sessionTitleSource: 'native' },
];

export function agentSessionTitleSource(id?: string | null) {
  return agentTypes.find(type => type.id === id)?.sessionTitleSource;
}

export function isAgentDisplayName(value: string) {
  return agentTypes.some(type => type.label.toLocaleLowerCase() === value.toLocaleLowerCase());
}

export function builtinAgentType(id?: string): AowAgentType | undefined {
  return agentTypes.find(type => type.id === id)?.id;
}

export function aowAgentType(agent: AowAgent): AowAgentType | undefined {
  return agent.agent_type ?? builtinAgentType(agent.id);
}

export function suggestedAgentExecutable(agents: AowAgent[], type: AowAgentType | '' | null): string {
  if (!type) return '';
  const detected = agents.find(agent => aowAgentType(agent) === type && agent.source === 'detected' && agent.executable);
  if (detected?.executable) return detected.executable;
  const configured = agents.find(agent => aowAgentType(agent) === type && agent.source === 'configured');
  return configured?.executable ?? configured?.command ?? '';
}
