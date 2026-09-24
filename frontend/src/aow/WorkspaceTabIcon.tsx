import { CalendarClock, Files, GitBranch, GitPullRequest, MessageSquare, MousePointerClick } from 'lucide-react';
import type { TaskKind } from '../features/automations/types';
import type { TerminalTabPresentation } from '../features/terminals/terminalPresentation';
import { FileTypeIcon } from '../features/files/FileTypeIcon';
import { TerminalTabIcon } from '../features/terminals/TerminalTabPreview';

export function WorkspaceTabIcon({ kind, path, terminal, automationKind }: {
  kind: string;
  path: string;
  terminal?: TerminalTabPresentation;
  automationKind?: TaskKind;
}) {
  if (kind === 'terminal' || kind === 'agent') return <TerminalTabIcon summary={terminal} />;
  if (kind === 'diff') return <GitBranch />;
  if (kind === 'session') return <MessageSquare />;
  if (kind === 'automation') return automationKind === 'manual' ? <MousePointerClick /> : <CalendarClock />;
  if (kind === 'pullRequest') return <GitPullRequest />;
  if (kind === 'browser') return <Files />;
  return <FileTypeIcon path={path} />;
}
