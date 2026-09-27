import { terminalLifecycleLabels, type TerminalLifecycle } from './terminalState';
import './terminal-state.css';

export function TerminalLifecycleDot({ state, className }: { state: TerminalLifecycle; className: string }) {
  return <span className={`terminal-lifecycle-dot ${className} ${state}`} title={terminalLifecycleLabels[state]} aria-hidden="true" />;
}
