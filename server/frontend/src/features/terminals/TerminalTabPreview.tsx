import { useEffect } from 'react';
import { createPortal } from 'react-dom';
import { Columns2, SquareTerminal } from 'lucide-react';
import type { TerminalTabPresentation } from './terminalPresentation';
import { AgentIcon } from '../agents/AgentIcon';

export function TerminalTabIcon({ summary }: { summary?: TerminalTabPresentation }) {
  return summary?.agentId ? <AgentIcon agentId={summary.agentId} />
    : summary && summary.agentCount > 1 ? <Columns2 /> : <SquareTerminal />;
}

export function TerminalTabPreview({ id, summary, left, top, onDismiss }: {
  id: string;
  summary: TerminalTabPresentation;
  left: number;
  top: number;
  onDismiss: () => void;
}) {
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      onDismiss();
    };
    window.addEventListener('keydown', escape, true);
    window.addEventListener('scroll', onDismiss, true);
    window.addEventListener('resize', onDismiss);
    return () => {
      window.removeEventListener('keydown', escape, true);
      window.removeEventListener('scroll', onDismiss, true);
      window.removeEventListener('resize', onDismiss);
    };
  }, [onDismiss]);

  return createPortal(
    <div id={id} role="tooltip" className="terminal-tab-preview" style={{ left, top }}>
      {summary.panes.map(pane => <div key={pane.id} className="terminal-tab-preview-pane">
        {pane.agentId ? <AgentIcon agentId={pane.agentId} /> : <SquareTerminal />}
        <span>{pane.title}</span>
      </div>)}
    </div>,
    document.body,
  );
}
