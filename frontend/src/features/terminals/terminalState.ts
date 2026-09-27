import type { TerminalPane, TerminalTab } from './types';

export const terminalLifecycleLabels = {
  starting: '初始化中', running: '运行中', failed: '启动失败',
  exited: '已退出', interrupted: '已中断', closed: '已关闭',
} as const;
export type TerminalLifecycle = keyof typeof terminalLifecycleLabels;
export const terminalDisplayLabels = { shown: '已显示', hidden: '已隐藏' } as const;
export const terminalControlLabels = { controlled: '已接管', observing: '旁观中', disconnected: '未连接' } as const;
export type TerminalControl = keyof typeof terminalControlLabels;

// Emulator attachment progress includes replaying a finished process. UI control
// state is derived separately and never treats that replay as taking control.
export type TerminalConnectionState = 'connecting' | 'connected' | 'observing' | 'waiting' | 'disconnected' | 'exited' | 'interrupted';

const connectionLabels: Record<TerminalConnectionState, string> = {
  connecting: '连接中', connected: terminalControlLabels.controlled, observing: terminalControlLabels.observing,
  waiting: '此终端正在其他窗口使用', disconnected: '连接中断',
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
  if (lifecycle === 'failed') return `${terminalLifecycleLabels[lifecycle]} · ${detail} · ${pane.agent_terminal?.error || '请查看终端输出'}`;
  return detail;
}
