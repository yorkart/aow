import { t } from '../../lib/i18n';
import { useCallback, useMemo, useRef, useState } from 'react';
import { SquareTerminal } from 'lucide-react';
import type { TerminalTab } from './types';
import type { AowWorktree } from '../../aow/types';
import { terminalTabPresentation } from './terminalPresentation';
import { terminalLifecycleLabels } from './terminalState';
import { requestTerminalPaneSelection, useTerminalStates } from './terminalViewState';
import { useInstanceGraph } from '../instances/useInstanceGraph';
import { indexTerminalChildren, TerminalDescendants } from './TerminalDescendants';
import { TaskCompletion } from '../../components/TaskCompletion';
import { useUnreadTerminalIds } from '../notifications/taskNotifications';
import { AgentIcon } from '../agents/AgentIcon';
import { AowListRow } from '../../components/AowListRow';
import { useConfirmation } from '../../components/ConfirmationDialog';
import { TerminalContextMenu } from './TerminalContextMenu';
import type { InstanceSort } from '../instances/InstanceScopeMenu';
import { InstancePanel } from '../instances/InstancePanel';
import './terminal-panel.css';

function terminalCreatedAt(tab: TerminalTab) {
  const timestamp = typeof tab.created_at === 'number' ? tab.created_at : Date.parse(tab.created_at ?? '');
  return Number.isFinite(timestamp) ? timestamp : Number.MAX_SAFE_INTEGER;
}

export function TerminalPanel({ tabs, descendantTabs = tabs, detectedAgents, titles, openedIds, activeId, onOpen, onOpenPane, onOpenFloating, canOpenFloating, onTerminate, onRebuild, onReload,
  worktrees, activeWorktreePath, showAll = false, onShowAllChange,
  sortBy = 'branch', onSortChange, loading = false, error, menuContainer }: {
  tabs: TerminalTab[];
  descendantTabs?: TerminalTab[];
  detectedAgents: Record<string, string | null>;
  titles: Record<string, string>;
  openedIds: Set<string>;
  activeId?: string;
  onOpen: (tab: TerminalTab) => void;
  onOpenPane?: (tab: TerminalTab, paneId: string) => void;
  onOpenFloating?: (tab: TerminalTab) => void;
  canOpenFloating?: (tab: TerminalTab) => boolean;
  onTerminate: (id: string) => Promise<void>;
  onRebuild: (id: string) => Promise<void>;
  onReload: () => void;
  worktrees?: AowWorktree[];
  activeWorktreePath?: string;
  showAll?: boolean;
  onShowAllChange?: (showAll: boolean) => void;
  sortBy?: InstanceSort;
  onSortChange?: (sortBy: InstanceSort) => void;
  loading?: boolean;
  error?: string;
  menuContainer?: HTMLElement;
}) {
  const terminalState = useTerminalStates();
  const unreadTerminalIds = useUnreadTerminalIds();
  const graph = useInstanceGraph();
  const childrenIndex = useMemo(() => indexTerminalChildren(descendantTabs, graph.items), [descendantTabs, graph.items]);
  const openChild = (tab: TerminalTab, paneId: string) => {
    if (onOpenPane) onOpenPane(tab, paneId);
    else { requestTerminalPaneSelection(tab.id, paneId); onOpen(tab); }
  };
  const [contextMenu, setContextMenu] = useState<{ tabId: string; x: number; y: number }>();
  const dismissMenu = useCallback(() => setContextMenu(undefined), []);
  const menuTab = contextMenu ? tabs.find(tab => tab.id === contextMenu.tabId) : undefined;
  const rebuildingIds = useRef(new Set<string>());
  const [rebuilding, setRebuilding] = useState<Set<string>>(new Set());
  const [rebuildError, setRebuildError] = useState('');
  const rebuild = async (id: string) => {
    if (rebuildingIds.current.has(id)) return;
    rebuildingIds.current.add(id);
    setRebuilding(new Set(rebuildingIds.current));
    setRebuildError('');
    try { await onRebuild(id); }
    catch (reason) { setRebuildError(reason instanceof Error ? reason.message : String(reason)); }
    finally {
      rebuildingIds.current.delete(id);
      setRebuilding(new Set(rebuildingIds.current));
    }
  };
  const { confirm, confirmationDialog } = useConfirmation();
  const terminate = async (tab: TerminalTab) => {
    const title = terminalTabPresentation(tab, detectedAgents, titles).title;
    if (await confirm({ title: t("terminals.destroy_terminal"), description: t("terminals.destroy_and_all_its_panes", { value0: title }),
      warning: t("terminals.this_ends_running_processes_and_tasks_worktrees_and_saved"), confirmLabel: t("terminals.destroy"), danger: true })) {
      await onTerminate(tab.id);
    }
  };
  const renderTab = (tab: TerminalTab) => {
    const presentation = terminalTabPresentation(tab, detectedAgents, titles);
    const pane = tab.panes.find(pane => pane.agent_terminal) ?? tab.panes[0];
    const state = terminalState(tab, openedIds.has(tab.id));
    return <TerminalDescendants key={tab.id} tab={tab} title={presentation.title} index={childrenIndex}
      worktrees={worktrees} detectedAgents={detectedAgents} titles={titles} activeId={activeId} onOpen={openChild}
      unreadTerminalIds={unreadTerminalIds}>
      <AowListRow className={`terminal-panel-row ${state.lifecycle}${tab.id === activeId ? ' active' : ''}`}
      openClassName="terminal-panel-open" key={tab.id}
      title={presentation.title} tooltip={`${presentation.title}\n${terminalLifecycleLabels[state.lifecycle]}`}
      icon={<AgentIcon agentId={presentation.agentId ?? pane?.agent_id} />} onOpen={() => onOpen(tab)}
      menuLabel={t("terminals.terminal_actions_2")} menuExpanded={contextMenu?.tabId === tab.id}
      onMenu={(x, y) => { setContextMenu({ tabId: tab.id, x, y }); }}>
      {unreadTerminalIds.has(tab.id) && <TaskCompletion />}
    </AowListRow></TerminalDescendants>;
  };
  return <section className="terminal-panel">
    {graph.error && <p className="terminal-panel-error" role="alert">{graph.error}</p>}
    {rebuildError ? <p className="terminal-panel-error" role="alert">{rebuildError}</p> : null}
    <InstancePanel title={t("terminals.terminal")} icon={<SquareTerminal />} items={tabs}
      loading={!!worktrees && showAll && loading} error={worktrees && showAll ? error : undefined}
      worktrees={worktrees} activeWorktreePath={activeWorktreePath} showAll={showAll} sortBy={sortBy}
      onShowAllChange={onShowAllChange} onSortChange={onSortChange} onScopeMenuOpen={dismissMenu}
      onReload={() => { onReload(); graph.reload(); }} noun={t("terminals.terminal")}
      itemKey={tab => tab.id} itemWorkspace={tab => tab.workspace_root} itemCreatedAt={terminalCreatedAt}
      renderItem={renderTab} menuContainer={menuContainer} />
    {contextMenu && menuTab ? <TerminalContextMenu container={menuContainer} x={contextMenu.x} y={contextMenu.y}
      title={terminalTabPresentation(menuTab, detectedAgents, titles).title} onClose={dismissMenu}
      onOpen={() => onOpen(menuTab)} onOpenFloating={onOpenFloating && (canOpenFloating?.(menuTab) ?? true) ? () => onOpenFloating(menuTab) : undefined}
      onRebuild={menuTab.panes.length > 0 && menuTab.panes.every(pane => pane.status !== 'running')
        ? () => void rebuild(menuTab.id) : undefined} rebuilding={rebuilding.has(menuTab.id)}
      onDestroy={() => void terminate(menuTab)} /> : null}
    {confirmationDialog}
  </section>;
}
