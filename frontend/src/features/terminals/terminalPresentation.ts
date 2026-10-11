import type { TerminalAgentProcess, TerminalLayout, TerminalPane, TerminalPaneActivity, TerminalTab } from './types';
import { agentTypes } from '../agents/agentTypes';

export function isCliTerminal(tab: TerminalTab) {
  return tab.panes.some(pane => pane.agent_terminal !== undefined);
}

export function terminalPaneAgent(pane: TerminalPane, detected: Record<string, string | null>) {
  if (pane.status !== 'running') return undefined;
  return Object.hasOwn(detected, pane.id) ? detected[pane.id]
    : pane.agent_id;
}

function agentTitle(pane: TerminalPane, detected: Record<string, string | null>, titles: Record<string, string>) {
  return pane.status === 'running' && terminalPaneAgent(pane, detected) ? titles[pane.id] : undefined;
}

export function defaultTerminalPaneName(pane: Pick<TerminalPane, 'cwd' | 'shell'>) {
  return pane.cwd.split(/[\\/]/).filter(Boolean).at(-1) || pane.cwd.trim()
    || pane.shell.split(/[\\/]/).filter(Boolean).at(-1) || 'Shell';
}

function agentFallbackTitle(agentId: string) {
  return agentTypes.find(agent => agent.id === agentId)?.label ?? agentId;
}

export function terminalPaneTitle(
  pane: TerminalPane,
  detected: Record<string, string | null>,
  titles: Record<string, string>,
  activity: Record<string, TerminalPaneActivity> = {},
  processes: Record<string, TerminalAgentProcess> = {},
) {
  const agentId = terminalPaneAgent(pane, detected);
  if (pane.status === 'running' && agentId && processes[pane.id]) {
    return titles[pane.id] || agentFallbackTitle(agentId);
  }
  const foregroundCommand = pane.status === 'running' ? activity[pane.id]?.foreground_command : undefined;
  if (foregroundCommand) return foregroundCommand;
  const cwd = activity[pane.id]?.cwd;
  if (pane.status === 'running' && !cwd && titles[pane.id]) return titles[pane.id];
  return defaultTerminalPaneName({ ...pane, cwd: cwd || pane.cwd });
}

function orderedTerminalPanes(tab: TerminalTab) {
  const ordered: TerminalPane[] = [];
  const seen = new Set<string>();
  const add = (id: string) => {
    const pane = tab.panes.find((item) => item.id === id);
    if (pane && !seen.has(id)) { seen.add(id); ordered.push(pane); }
  };
  const visit = (layout: TerminalLayout | null) => {
    if (!layout) return;
    if (layout.type === 'pane') add(layout.pane_id);
    else { visit(layout.first); visit(layout.second); }
  };
  visit(tab.layout);
  tab.panes.forEach((pane) => add(pane.id));
  return ordered;
}

export function terminalTabPresentation(tab: TerminalTab, detected: Record<string, string | null>, titles: Record<string, string>) {
  const panes = orderedTerminalPanes(tab).map(pane => ({
    id: pane.id, title: terminalPaneTitle(pane, detected, titles), agentId: terminalPaneAgent(pane, detected),
  }));
  const agents = panes.filter(pane => pane.agentId);
  const agentCount = agents.length;
  const agentIds = new Set(agents.map(pane => pane.agentId));
  let title = tab.name;
  // Unknown naming intent (old servers) preserves the existing label.
  if (tab.name_is_custom === false) {
    if (agentCount > 1) title = `${agentCount} agents`;
    else if (agentCount === 1) {
      const pane = tab.panes.find(pane => pane.id === agents[0].id)!;
      title = agentTitle(pane, detected, titles) || tab.name;
    }
  }
  return { title, agentCount, agentId: agentIds.size === 1 ? agents[0].agentId : undefined, panes };
}

function flattenedPaneTitle(
  tab: TerminalTab,
  pane: TerminalPane,
  paneCount: number,
  detected: Record<string, string | null>,
  titles: Record<string, string>,
  activity: Record<string, TerminalPaneActivity>,
  processes: Record<string, TerminalAgentProcess>,
) {
  const title = terminalPaneTitle(pane, detected, titles, activity, processes);
  if (paneCount > 1) return `${tab.name} · ${title}`;
  return tab.name_is_custom === false ? title : tab.name;
}

export type TerminalTabPresentation = ReturnType<typeof terminalTabPresentation>;

export interface TerminalDimensions { cols: number; rows: number }
export interface TerminalFrame extends TerminalDimensions { width: number; height: number; top: number; cursorRow: number }
export interface TerminalPaneControls {
  send: (text: string, paste?: boolean) => boolean;
  submit: (text: string) => boolean;
  takeControl: () => void;
  focus: () => void;
  scroll: (lines: number | 'bottom', point?: { clientX: number; clientY: number }) => void;
}

export function savedTerminalDimensions(pane: Pick<TerminalPane, 'cols' | 'rows'>): TerminalDimensions {
  const valid = (value: number | undefined) => Number.isInteger(value) && value! > 0 && value! <= 1000;
  return { cols: valid(pane.cols) ? pane.cols! : 80, rows: valid(pane.rows) ? pane.rows! : 24 };
}

export function flattenTerminalTabs(
  tabs: TerminalTab[],
  detected: Record<string, string | null> = {},
  titles: Record<string, string> = {},
  activity: Record<string, TerminalPaneActivity> = {},
  processes: Record<string, TerminalAgentProcess> = {},
) {
  return tabs.flatMap((tab) => {
    const ordered = orderedTerminalPanes(tab);
    return ordered.map((pane) => ({
      key: `${tab.id}:${pane.id}`, tab, pane,
      title: flattenedPaneTitle(tab, pane, ordered.length, detected, titles, activity, processes),
    }));
  });
}

export const terminalAccessoryKeys = [
  { label: 'Tab', data: '\t' }, { label: 'Enter', data: '\r' },
  { label: 'Shift+Tab', data: '\u001b[Z' }, { label: 'Space', data: ' ' },
  { label: 'Ctrl+C', data: '\u0003' }, { label: 'Esc', data: '\u001b' },
  { label: '↑', data: '\u001b[A' }, { label: '↓', data: '\u001b[B' },
  { label: '←', data: '\u001b[D' }, { label: '→', data: '\u001b[C' },
  { label: '⌫', data: '\u007f' }, { label: 'Ctrl+D', data: '\u0004' },
] as const;
