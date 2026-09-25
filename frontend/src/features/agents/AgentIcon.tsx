import { Bot } from 'lucide-react';
import codexIcon from '../../assets/agents/codex.png';
import claudeIcon from '../../assets/agents/claude.png';
import traeIcon from '../../assets/agents/trae.png';
import hermesIcon from '../../assets/agents/hermes.png';

const icons = new Map([
  ['codex', codexIcon],
  ['claude', claudeIcon],
  ['claude-code', claudeIcon],
  ['traecli', traeIcon],
  ['hermes', hermesIcon],
]);

export function AgentIcon({ agentId }: { agentId?: string | null }) {
  const icon = icons.get(agentId?.toLowerCase() ?? '');
  return icon
    ? <img className="agent-icon" src={icon} alt="" aria-hidden="true" draggable={false} />
    : <Bot className="agent-icon" aria-hidden="true" />;
}
