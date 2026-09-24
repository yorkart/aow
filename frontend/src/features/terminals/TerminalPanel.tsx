import { useCallback, useId, useRef, useState } from 'react';
import { ChevronRight, Eye, EyeOff, Glasses, MoreHorizontal, RefreshCw, ShieldCheck, SquareTerminal, Unplug, UserRound } from 'lucide-react';
import type { TerminalTab } from './types';
import type { AowWorktree } from '../../aow/types';
import { isCliTerminal, terminalTabPresentation } from './terminalPresentation';
import { terminalControlLabels, terminalDisplayLabels, terminalLifecycleLabels } from './terminalState';
import { useTerminalStates } from './terminalViewState';
import { TerminalLifecycleDot } from './TerminalLifecycleDot';
import { AgentIcon } from '../agents/AgentIcon';
import { AowIconButton } from '../../components/AowIconButton';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { AowListRow } from '../../components/AowListRow';
import { useConfirmation } from '../../components/ConfirmationDialog';
import { TerminalContextMenu } from './TerminalContextMenu';
import { TerminalScopeMenu, type TerminalSort } from './TerminalScopeMenu';
import './terminal-panel.css';

const terminalGroups = [
  { id: 'user', cli: false, title: 'User Terminals', icon: UserRound, tooltip: '用户创建' },
  { id: 'cli', cli: true, title: 'CLI Terminals', icon: SquareTerminal, tooltip: <><code>aow-cli agent</code> 创建</> },
] as const;

type TerminalGroup = typeof terminalGroups[number]['id'];
const terminalDisplayIcons = { shown: Eye, hidden: EyeOff };
const terminalControlIcons = { observing: Glasses, controlled: ShieldCheck, disconnected: Unplug };

function worktreeName(worktree: AowWorktree) {
  return worktree.detached ? 'detached'
    : worktree.branch || worktree.path.split('/').filter(Boolean).at(-1) || worktree.path;
}

function terminalCreatedAt(tab: TerminalTab) {
  const timestamp = typeof tab.created_at === 'number' ? tab.created_at : Date.parse(tab.created_at ?? '');
  return Number.isFinite(timestamp) ? timestamp : Number.MAX_SAFE_INTEGER;
}

export function TerminalPanel({ tabs, detectedAgents, titles, openedIds, activeId, onOpen, onOpenFloating, onTerminate, onRebuild, onReload,
  worktrees, showAll = { user: false, cli: false }, onShowAllChange,
  sortBy = { user: 'branch', cli: 'branch' }, onSortChange, loading = false, error, menuContainer }: {
  tabs: TerminalTab[];
  detectedAgents: Record<string, string | null>;
  titles: Record<string, string>;
  openedIds: Set<string>;
  activeId?: string;
  onOpen: (tab: TerminalTab) => void;
  onOpenFloating?: (tab: TerminalTab) => void;
  onTerminate: (id: string) => Promise<void>;
  onRebuild: (id: string) => Promise<void>;
  onReload: () => void;
  worktrees?: AowWorktree[];
  showAll?: Record<TerminalGroup, boolean>;
  onShowAllChange?: (group: TerminalGroup, showAll: boolean) => void;
  sortBy?: Record<TerminalGroup, TerminalSort>;
  onSortChange?: (group: TerminalGroup, sortBy: TerminalSort) => void;
  loading?: boolean;
  error?: string;
  menuContainer?: HTMLElement;
}) {
  const panelId = useId();
  const terminalState = useTerminalStates();
  const [collapsedGroups, setCollapsedGroups] = useState<Partial<Record<TerminalGroup, boolean>>>({});
  const [collapsedWorktrees, setCollapsedWorktrees] = useState<Record<string, boolean>>({});
  const [scopeMenu, setScopeMenu] = useState<{ anchor: HTMLButtonElement; group: TerminalGroup }>();
  const dismissScopeMenu = useCallback(() => setScopeMenu(undefined), []);
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
    if (await confirm({ title: '销毁终端？', description: `销毁 ${title} 及其全部窗格。`,
      warning: '此操作会结束正在运行的进程和任务，worktree 和已保存的会话记录会保留。', confirmLabel: '销毁', danger: true })) {
      await onTerminate(tab.id);
    }
  };
  const renderTab = (tab: TerminalTab) => {
    const presentation = terminalTabPresentation(tab, detectedAgents, titles);
    const pane = tab.panes.find(pane => pane.agent_terminal) ?? tab.panes[0];
    const state = terminalState(tab, openedIds.has(tab.id));
    const DisplayIcon = terminalDisplayIcons[state.display];
    const ControlIcon = terminalControlIcons[state.control];
    const displayLabel = terminalDisplayLabels[state.display];
    const controlLabel = terminalControlLabels[state.control];
    return <AowListRow className={`terminal-panel-row${tab.id === activeId ? ' active' : ''}`}
      openClassName="terminal-panel-open" key={tab.id}
      title={presentation.title} tooltip={presentation.title}
      icon={<AgentIcon agentId={presentation.agentId ?? pane?.agent_id} />} onOpen={() => onOpen(tab)}
      menuLabel="终端操作" menuExpanded={contextMenu?.tabId === tab.id}
      onMenu={(x, y) => { dismissScopeMenu(); setContextMenu({ tabId: tab.id, x, y }); }}>
      <small className="terminal-panel-status">
        <TerminalLifecycleDot className="terminal-panel-status-dot" state={state.lifecycle} />
        <span className="terminal-panel-status-label">{terminalLifecycleLabels[state.lifecycle]}</span>
        <span className="terminal-panel-status-separator" aria-hidden="true">·</span>
        <DisplayIcon className={state.display === 'shown' ? 'terminal-panel-state-active' : undefined}
          role="img" aria-label={displayLabel}><title>{displayLabel}</title></DisplayIcon>
        <span className="terminal-panel-status-separator" aria-hidden="true">·</span>
        <ControlIcon className={state.control === 'controlled' ? 'terminal-panel-state-active' : undefined}
          role="img" aria-label={controlLabel}><title>{controlLabel}</title></ControlIcon>
      </small>
    </AowListRow>;
  };
  return <section className="terminal-panel">
    {rebuildError ? <p className="terminal-panel-error" role="alert">{rebuildError}</p> : null}
    <AowPanelStack className="terminal-panel-groups">
      {terminalGroups.map(({ id, cli, title, icon: Icon, tooltip }) => {
        const items = tabs.filter(tab => isCliTerminal(tab) === cli);
        const grouped = showAll[id] && !!worktrees;
        const groupLoading = grouped && loading;
        const collapsed = collapsedGroups[id] ?? (items.length === 0 && !groupLoading && !(grouped && error));
        const bodyId = `${panelId}-${id}`;
        const branches = grouped ? [...worktrees]
          .sort((left, right) => Number(right.is_main) - Number(left.is_main)
            || worktreeName(left).localeCompare(worktreeName(right)))
          .map(worktree => ({ key: `worktree:${worktree.path}`, worktree, members: items.filter(tab => tab.workspace_root === worktree.path) }))
          .filter(group => group.members.length) : [];
        const sections = sortBy[id] === 'createdAt'
          ? branches.flatMap(group => group.members.map(tab => ({ ...group, key: `terminal:${tab.id}`, members: [tab] })))
            .sort((left, right) => terminalCreatedAt(left.members[0]) - terminalCreatedAt(right.members[0]))
          : branches;
        return <AowPanel key={id} className="terminal-panel-group" bodyClassName="terminal-panel-body" title={title} tooltip={tooltip} icon={<Icon />} collapsed={collapsed} controlsId={bodyId}
            onCollapsedChange={next => setCollapsedGroups(groups => ({ ...groups, [id]: next }))}
            details={grouped ? <span className="terminal-panel-count">{items.length}</span> : undefined}
            actions={<>
              <AowIconButton title="刷新终端列表" aria-label={`刷新 ${title}`} onClick={onReload}><RefreshCw /></AowIconButton>
              {worktrees && onShowAllChange ? <AowIconButton title="终端显示范围" aria-label={`${title} 显示范围`}
                aria-haspopup="menu" aria-expanded={scopeMenu?.group === id}
                className={grouped ? 'terminal-scope-active' : undefined}
                onClick={event => {
                  dismissMenu();
                  const anchor = event.currentTarget;
                  setScopeMenu(current => current?.group === id ? undefined : { anchor, group: id });
                }}><MoreHorizontal /></AowIconButton> : null}
            </>}>
            {grouped && error ? <p className="terminal-panel-error" role="alert">{error}</p> : null}
            {groupLoading ? <p className="terminal-panel-empty" role="status">正在加载所有终端…</p> : null}
            {!items.length && !groupLoading ? <p className="terminal-panel-empty">暂无终端</p> : grouped ? sections
              .map(({ key: sectionKey, worktree, members }, index) => {
                const key = `${id}:${sectionKey}`;
                const collapsed = collapsedWorktrees[key] ?? false;
                const groupId = `${bodyId}-worktree-${index}`;
                const name = worktreeName(worktree);
                return <section className="terminal-worktree-group" key={sectionKey} aria-label={worktree.path}>
                  <button className="terminal-worktree-heading" title={`${worktree.path}${worktree.branch ? `\n${worktree.branch}` : ''}`}
                    aria-expanded={!collapsed} aria-controls={groupId}
                    onClick={() => setCollapsedWorktrees(groups => ({ ...groups, [key]: !groups[key] }))}>
                    <ChevronRight className={collapsed ? undefined : 'expanded'} />
                    <span>{worktree.is_main ? '主仓库 · ' : ''}{name}</span>
                    <small className="terminal-panel-count">{members.length}</small>
                  </button>
                  <div id={groupId} hidden={collapsed}>{members.map(renderTab)}</div>
                </section>;
              }) : items.map(renderTab)}
        </AowPanel>;
      })}
    </AowPanelStack>
    {scopeMenu && worktrees && onShowAllChange ? <TerminalScopeMenu container={menuContainer} anchor={scopeMenu.anchor} checked={showAll[scopeMenu.group]}
      sortBy={sortBy[scopeMenu.group]} onSortChange={value => onSortChange?.(scopeMenu.group, value)}
      onChange={value => onShowAllChange(scopeMenu.group, value)} onClose={dismissScopeMenu} /> : null}
    {contextMenu && menuTab ? <TerminalContextMenu container={menuContainer} x={contextMenu.x} y={contextMenu.y}
      title={terminalTabPresentation(menuTab, detectedAgents, titles).title} onClose={dismissMenu}
      onOpen={() => onOpen(menuTab)} onOpenFloating={onOpenFloating ? () => onOpenFloating(menuTab) : undefined}
      onRebuild={menuTab.panes.length > 0 && menuTab.panes.every(pane => pane.status !== 'running')
        ? () => void rebuild(menuTab.id) : undefined} rebuilding={rebuilding.has(menuTab.id)}
      onDestroy={() => void terminate(menuTab)} /> : null}
    {confirmationDialog}
  </section>;
}
