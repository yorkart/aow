import { Globe, Inbox, Settings, CalendarClock, Files, GitBranch, GitPullRequest, MessageSquare, MousePointerClick } from 'lucide-react';
import { AcpIcon } from '../features/agents/AcpIcon';
import { AgentIcon } from '../features/agents/AgentIcon';
import type { TaskKind } from '../features/automations/types';
import type { TerminalTabPresentation } from '../features/terminals/terminalPresentation';
import { FileTypeIcon } from '../features/files/FileTypeIcon';
import { TerminalTabIcon } from '../features/terminals/TerminalTabPreview';

export function WorkspaceTabIcon({ kind, path, terminal, automationKind, acpAgentId }: {
  kind: string;
  path: string;
  terminal?: TerminalTabPresentation;
  automationKind?: TaskKind;
  acpAgentId?: string;
}) {
  if (kind === 'terminal' || kind === 'agent') return <TerminalTabIcon summary={terminal} />;
  if (kind === 'global') return <Globe />;
  if (kind === 'inbox') return <Inbox />;
  if (kind === 'settings') return <Settings />;
  if (kind === 'diff') return <GitBranch />;
  if (kind === 'session') return <MessageSquare />;
  if (kind === 'acp') return acpAgentId ? <AgentIcon agentId={acpAgentId.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} /> : <AcpIcon />;
  if (kind === 'automation') return automationKind === 'manual' ? <MousePointerClick /> : <CalendarClock />;
  if (kind === 'pullRequest') return <GitPullRequest />;
  if (kind === 'browser') return <Files />;
  return <FileTypeIcon path={path} />;
}
