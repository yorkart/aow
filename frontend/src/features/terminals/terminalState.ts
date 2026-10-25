import { t } from '../../lib/i18n';
import type { TerminalPane, TerminalTab } from './types';

export const terminalLifecycleLabels = {
  starting: t("terminals.initializing"), running: t("pr.running"), failed: t("pr.failed_to_start"),
  exited: t("terminals.exited"), interrupted: t("aow.interrupted"), closed: t("pr.closed"),
} as const;
export type TerminalLifecycle = keyof typeof terminalLifecycleLabels;
export const terminalDisplayLabels = { shown: t("terminals.visible"), hidden: t("terminals.hidden") } as const;
export const terminalControlLabels = { controlled: t("terminals.controlling"), observing: t("terminals.observing"), disconnected: t("terminals.disconnected") } as const;
export type TerminalControl = keyof typeof terminalControlLabels;

// Emulator attachment progress includes replaying a finished process. UI control
// state is derived separately and never treats that replay as taking control.
export type TerminalConnectionState = 'connecting' | 'connected' | 'observing' | 'waiting' | 'disconnected' | 'exited' | 'interrupted';

const connectionLabels: Record<TerminalConnectionState, string> = {
  connecting: t("terminals.connecting"), connected: terminalControlLabels.controlled, observing: terminalControlLabels.observing,
  waiting: t("terminals.this_terminal_is_in_use_in_another_window"), disconnected: t("terminals.connection_interrupted"),
  exited: terminalLifecycleLabels.exited, interrupted: terminalLifecycleLabels.interrupted,
};

export function terminalPaneLifecycle(pane: TerminalPane): TerminalLifecycle {
  if (pane.status !== 'running') return pane.status;
  const phase = pane.agent_terminal?.phase;
  return phase === 'starting' || phase === 'failed' ? phase : 'running';
}

export function terminalTabLifecycle(tab: TerminalTab): TerminalLifecycle {
  const states = tab.panes.map(terminalPaneLifecycle);
  // Prefer live panes over finished ones, independent of pane order. Report an
  // interruption rather than a normal exit once every pane has finished.
  return (['running', 'starting', 'failed', 'interrupted', 'exited'] as const)
    .find(state => states.includes(state)) ?? 'closed';
}

export function terminalPaneControl(pane: TerminalPane, connection?: TerminalConnectionState): TerminalControl {
  if (pane.status !== 'running') return 'disconnected';
  return connection === 'connected' ? 'controlled' : connection === 'observing' ? 'observing' : 'disconnected';
}

export function terminalTabState(tab: TerminalTab, opened: boolean, connection: (paneId: string) => TerminalConnectionState | undefined) {
  const panes = tab.panes.map(pane => ({ pane, connection: connection(pane.id) }));
  const controls = panes.map(({ pane, connection }) => terminalPaneControl(pane, connection));
  return {
    lifecycle: terminalTabLifecycle(tab),
    // Opened means a tab exists, even when it is in the background or disconnected.
    display: opened || panes.some(pane => pane.connection !== undefined) ? 'shown' : 'hidden',
    control: controls.includes('controlled') ? 'controlled' : controls.includes('observing') ? 'observing' : 'disconnected',
  } as const;
}

export function terminalPaneStatusMessage(pane: TerminalPane, connection: TerminalConnectionState, message?: string) {
  // A stream can report exit before the parent's process snapshot is updated.
  const lifecycle = connection === 'exited' || connection === 'interrupted' ? connection : terminalPaneLifecycle(pane);
  if (lifecycle === 'exited' || lifecycle === 'interrupted') return terminalLifecycleLabels[lifecycle];
  const detail = message || connectionLabels[connection];
  if (lifecycle === 'starting') return `${terminalLifecycleLabels[lifecycle]} · ${detail}`;
  if (lifecycle === 'failed') return `${terminalLifecycleLabels[lifecycle]} · ${detail} · ${pane.agent_terminal?.error || t("common.check_terminal_output")}`;
  return detail;
}
