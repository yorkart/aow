import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties, HTMLAttributes, ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { ListX, Pencil, Plus, X } from 'lucide-react';
import type { TerminalTabPresentation } from '../features/terminals/terminalPresentation';
import type { AowAgent } from '../features/agents/types';
import type { TaskKind } from '../features/automations/types';
import { persist, readStored } from './floatingWorkspaceState';
import { TerminalTabPreview } from '../features/terminals/TerminalTabPreview';
import { WorkspaceTabIcon } from './WorkspaceTabIcon';
import { WorkspaceNewMenu } from './WorkspaceNewMenu';
import { WorkspaceMenuLabel, type WorkspaceMenuAction } from './WorkspaceMenuLabel';

export interface WorkspaceTab {
  id: string;
  kind: 'terminal' | 'agent' | 'file' | 'diff' | 'session' | 'automation' | 'pullRequest' | 'browser';
  label: string;
  targetId: string;
  title?: string;
  terminal?: TerminalTabPresentation;
  automationKind?: TaskKind;
  renameable?: boolean;
  dirty?: boolean;
  pending?: boolean;
}
export interface TabRevealRequest { tabId: string }
type WorkspaceTabGroup = Exclude<WorkspaceTab['kind'], 'agent'>;
const groupDefinitions: { id: WorkspaceTabGroup; label: string; color: string }[] = [
  { id: 'browser', label: '系统文件浏览器', color: '#8fa8bd' },
  { id: 'terminal', label: 'Terminal', color: '#6ca8f1' },
  { id: 'session', label: 'Conversation', color: '#ae9bd7' },
  { id: 'automation', label: '自动化', color: '#c8b480' },
  { id: 'pullRequest', label: 'PR', color: '#c99ab5' },
  { id: 'file', label: '文件', color: '#86b6a0' },
  { id: 'diff', label: 'Diff', color: '#c6a583' },
];
export function isWorkspaceTabKind(kind: unknown): kind is WorkspaceTab['kind'] {
  return kind === 'agent' || groupDefinitions.some(group => group.id === kind);
}
export function workspaceTabGroup(tab: WorkspaceTab): WorkspaceTabGroup {
  return tab.kind === 'agent' ? 'terminal' : tab.kind;
}
export function groupWorkspaceTabs(tabs: WorkspaceTab[]) {
  return groupDefinitions.map(group => ({ ...group, tabs: tabs.filter(tab => workspaceTabGroup(tab) === group.id) }))
    .filter(group => group.tabs.length);
}
interface TabMenuState { tab: WorkspaceTab; anchor: HTMLElement; x: number; y: number }

function CenterTabContextMenu({
  state, hasOtherTabs, onClose, onRename, onCloseTab, onCloseOthers, onCloseAll, onOpenElsewhere, destination,
}: {
  state: TabMenuState;
  onOpenElsewhere?: () => void;
  destination?: WorkspaceMenuAction;
  hasOtherTabs: boolean;
  onClose: () => void;
  onRename: () => void;
  onCloseTab: () => void;
  onCloseOthers: () => void;
  onCloseAll: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x: state.x, y: state.y });

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({
      x: Math.max(4, Math.min(state.x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(state.y, window.innerHeight - bounds.height - 4)),
    });
  }, [state.x, state.y]);

  useEffect(() => {
    const close = () => onClose();
    const anchorBounds = state.anchor.getBoundingClientRect();
    const closeOnScroll = () => {
      const bounds = state.anchor.getBoundingClientRect();
      // Focusing an overflowed tab may queue a scroll event before its menu opens.
      // Dismiss only when scrolling actually moves the anchor after opening.
      if (bounds.x !== anchorBounds.x || bounds.y !== anchorBounds.y) onClose();
    };
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('pointerdown', close);
    window.addEventListener('scroll', closeOnScroll, true);
    window.addEventListener('resize', close);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('scroll', closeOnScroll, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('keydown', key);
    };
  }, [onClose, state.anchor]);

  const action = (callback: () => void) => () => { callback(); onClose(); };

  const canRename = state.tab.renameable !== false && (state.tab.kind === 'terminal' || state.tab.kind === 'agent' || state.tab.kind === 'file');

  return <div ref={menu} className="project-aow-context-menu project-aow-tab-context-menu" style={{ left: position.x, top: position.y }} role="menu" aria-label={`${state.tab.label} 操作`} onPointerDown={(event) => event.stopPropagation()}>
    {onOpenElsewhere && destination ? <button role="menuitem" onClick={action(onOpenElsewhere)}><WorkspaceMenuLabel action={destination} /></button> : null}
    {canRename ? <button role="menuitem" onClick={action(onRename)}><Pencil />重命名</button> : null}
    <button role="menuitem" onClick={action(onCloseTab)}><X />关闭</button>
    <button role="menuitem" disabled={!hasOtherTabs} onClick={action(onCloseOthers)}><ListX />关闭其他</button>
    <button role="menuitem" onClick={action(onCloseAll)}><ListX />关闭所有</button>
  </div>;
}

interface Props {
  workspaceKey: string;
  revealRequest?: TabRevealRequest;
  tabs: WorkspaceTab[];
  activeId?: string;
  visible: boolean;
  agents: AowAgent[];
  onActivate: (tab: WorkspaceTab) => void;
  onCloseTab: (tab: WorkspaceTab) => void | Promise<unknown>;
  onCloseTabs: (tabs: WorkspaceTab[], selected: WorkspaceTab) => void | Promise<unknown>;
  onCanRename: (tab: WorkspaceTab) => boolean;
  onRename: (tab: WorkspaceTab, name: string) => void | Promise<unknown>;
  onError: (message: string) => void;
  destination: (tab: WorkspaceTab) => WorkspaceMenuAction | undefined;
  onOpenElsewhere: (tab: WorkspaceTab) => void;
  onOpenFile?: () => void;
  onOpenBrowser?: () => void;
  onCreateNote: (kind: 'md' | 'txt') => void;
  onCreateTerminal: (agent?: AowAgent) => void;
  leadingControls?: ReactNode;
  controls?: ReactNode;
  newLabel?: string;
  tabListLabel?: string;
  headerProps?: HTMLAttributes<HTMLElement>;
}

export function WorkspaceTabs({ workspaceKey, revealRequest, tabs, activeId, visible, agents, onActivate, onCloseTab, onCloseTabs,
  onCanRename, onRename, onError, destination, onOpenElsewhere, onOpenFile, onOpenBrowser, onCreateNote,
  onCreateTerminal, leadingControls, controls, newLabel = '新建窗体', tabListLabel = '工作区标签', headerProps }: Props) {
  const groups = useMemo(() => groupWorkspaceTabs(tabs), [tabs]);
  const storageKey = `aow-tab-groups-v1:${workspaceKey}`;
  const [collapsed, setCollapsed] = useState<Set<WorkspaceTabGroup>>(() => {
    const saved = readStored<unknown>(storageKey, []);
    return new Set(Array.isArray(saved) ? saved.filter(id => groupDefinitions.some(group => group.id === id)) : []);
  });
  const handledReveal = useRef<TabRevealRequest | undefined>(undefined);
  const scrolledReveal = useRef<TabRevealRequest | undefined>(undefined);
  const tabBarRef = useRef<HTMLDivElement>(null);
  const groupId = useId();
  useEffect(() => {
    const tabBar = tabBarRef.current;
    if (!tabBar) return;
    const scrollTabs = (event: WheelEvent) => {
      if (event.ctrlKey || event.metaKey || Math.abs(event.deltaY) <= Math.abs(event.deltaX)
        || tabBar.scrollWidth <= tabBar.clientWidth) return;
      const unit = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? 16
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE ? tabBar.clientWidth : 1;
      event.preventDefault();
      tabBar.scrollLeft += event.deltaY * unit;
    };
    // React's wheel listeners are passive; a native listener lets us prevent page scrolling.
    tabBar.addEventListener('wheel', scrollTabs, { passive: false });
    return () => tabBar.removeEventListener('wheel', scrollTabs);
  }, []);
  useEffect(() => persist(storageKey, [...collapsed]), [storageKey, collapsed]);
  useEffect(() => {
    // Only explicit open requests unfold a group. Restoring tabs and background
    // metadata updates must preserve the user's saved choice, even for the active tab.
    if (!revealRequest || handledReveal.current === revealRequest) return;
    const tab = tabs.find(tab => tab.id === revealRequest.tabId);
    if (!tab || tab.pending) return;
    handledReveal.current = revealRequest;
    setCollapsed(previous => {
      const group = workspaceTabGroup(tab);
      if (!previous.has(group)) return previous;
      const next = new Set(previous);
      next.delete(group);
      return next;
    });
  }, [revealRequest, tabs]);
  useLayoutEffect(() => {
    if (!visible || !revealRequest || scrolledReveal.current === revealRequest) return;
    if (tabs.find(tab => tab.id === revealRequest.tabId)?.pending) return;
    const element = [...tabBarRef.current?.querySelectorAll<HTMLElement>('[data-workspace-tab-id]') ?? []]
      .find(element => element.dataset.workspaceTabId === revealRequest.tabId);
    if (!element || !element.getClientRects().length) return;
    element.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    scrolledReveal.current = revealRequest;
  }, [visible, revealRequest, collapsed, tabs]);
  const [newMenu, setNewMenu] = useState(false);
  const [contextMenu, setContextMenu] = useState<TabMenuState>();
  const [rename, setRename] = useState<{ tabId: string; draft: string; original: string }>();
  const [preview, setPreview] = useState<{ tabId: string; left: number; top: number }>();
  const previewId = useId();
  const newMenuRef = useRef<HTMLDivElement>(null);
  const cancelRename = useRef(false);
  const dismissPreview = useCallback(() => setPreview(undefined), []);
  const dismissMenu = useCallback(() => setContextMenu(undefined), []);
  const summary = preview ? tabs.find(tab => tab.id === preview.tabId)?.terminal : undefined;
  const run = (action: () => void | Promise<unknown>) => { void Promise.resolve().then(action).catch(error => onError(error instanceof Error ? error.message : String(error))); };

  useEffect(() => {
    if (visible) return;
    setNewMenu(false); setContextMenu(undefined); setPreview(undefined);
  }, [visible]);
  useEffect(() => {
    if (rename && !tabs.some(tab => tab.id === rename.tabId)) setRename(undefined);
    if (contextMenu && !tabs.some(tab => tab.id === contextMenu.tab.id)) setContextMenu(undefined);
  }, [tabs, rename, contextMenu]);
  useEffect(() => {
    if (!newMenu) return;
    const closeOutside = (event: PointerEvent) => { if (!newMenuRef.current?.contains(event.target as Node)) setNewMenu(false); };
    const close = () => setNewMenu(false);
    const closeWhenHidden = () => { if (document.hidden) close(); };
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    document.addEventListener('pointerdown', closeOutside);
    document.addEventListener('visibilitychange', closeWhenHidden);
    window.addEventListener('blur', close);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    window.addEventListener('keydown', key);
    return () => {
      document.removeEventListener('pointerdown', closeOutside);
      document.removeEventListener('visibilitychange', closeWhenHidden);
      window.removeEventListener('blur', close);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('keydown', key);
    };
  }, [newMenu]);
  const showPreview = (tab: WorkspaceTab, element: HTMLElement) => {
    if (!tab.terminal || tab.terminal.panes.length < 2 || rename) return;
    const bounds = element.getBoundingClientRect();
    setPreview({ tabId: tab.id, left: Math.max(8, Math.min(bounds.left, window.innerWidth - 368)), top: bounds.bottom + 6 });
  };
  const beginRename = (tab: WorkspaceTab) => {
    if (tab.renameable === false || (tab.kind !== 'terminal' && tab.kind !== 'agent' && tab.kind !== 'file')) return;
    if (!onCanRename(tab)) return;
    cancelRename.current = false;
    dismissPreview(); setContextMenu(undefined); setNewMenu(false); onActivate(tab);
    setRename({ tabId: tab.id, draft: tab.label, original: tab.label });
  };
  const finishRename = () => {
    const tab = tabs.find(tab => tab.id === rename?.tabId);
    setRename(undefined);
    if (cancelRename.current || !tab || !rename) return;
    const name = rename.draft.trim();
    if (name === rename.original.trim()) return;
    if (!name) { onError('名称不能为空。'); return; }
    run(() => onRename(tab, name));
  };
  const toggleGroup = (group: WorkspaceTabGroup) => {
    dismissPreview(); setContextMenu(undefined); setNewMenu(false);
    setCollapsed(previous => {
      const next = new Set(previous);
      if (next.has(group)) next.delete(group); else next.add(group);
      return next;
    });
  };
  const closeGroup = (selected: WorkspaceTab, keep: boolean) => run(async () => {
    const group = workspaceTabGroup(selected);
    const targets = tabs.filter(tab => workspaceTabGroup(tab) === group && (!keep || tab.id !== selected.id));
    if (!targets.length) return;
    const dirty = targets.filter(tab => tab.dirty).length;
    if (group === 'file' && dirty && !window.confirm(`有 ${dirty} 个文件包含未保存的更改，确定关闭吗？`)) return;
    await onCloseTabs(targets, selected);
  });

  return <>
    <nav {...headerProps} className={`project-aow-center-tabs${headerProps?.className ? ` ${headerProps.className}` : ''}`}>
      {leadingControls}
      <div ref={tabBarRef} className="project-aow-center-tab-scroll" role="tablist" aria-label={tabListLabel}>
        {groups.map(group => {
          const folded = collapsed.has(group.id);
          const activeTab = group.tabs.find(tab => tab.id === activeId);
          const action = folded ? '展开' : '收起';
          return <div key={group.id} className={`project-aow-tab-group${folded ? ' collapsed' : ''}`}
            role="presentation" style={{ '--tab-group-color': group.color } as CSSProperties}>
            <button type="button" className={`project-aow-group-toggle${activeTab ? ' active' : ''}`}
              aria-label={`${action} ${group.label} 分组（${group.tabs.length} 个 Tab）`} aria-expanded={!folded}
              aria-controls={`${groupId}-${group.id}`} title={`${group.label} · ${group.tabs.length} 个 Tab · 点击${action}${folded && activeTab ? `\n当前：${activeTab.label}` : ''}`}
              onClick={() => toggleGroup(group.id)}>
              <WorkspaceTabIcon kind={group.id} path="" />
              {folded ? <span className="project-aow-group-count" aria-hidden="true">{group.tabs.length > 9 ? '…' : group.tabs.length}</span> : null}
            </button>
            <div id={`${groupId}-${group.id}`} className="project-aow-group-tabs" role="presentation">
              {!folded ? group.tabs.map(tab => <div key={tab.id} data-workspace-tab-id={tab.id} role="tab" tabIndex={0} aria-selected={activeId === tab.id}
                className={`project-aow-center-tab${activeId === tab.id ? ' active' : ''}`} title={tab.title ?? tab.label}
                aria-describedby={preview?.tabId === tab.id ? previewId : undefined}
                onMouseEnter={event => showPreview(tab, event.currentTarget)} onMouseLeave={dismissPreview}
                onFocus={event => { if (event.target === event.currentTarget) showPreview(tab, event.currentTarget); }} onBlur={dismissPreview}
                onClick={() => onActivate(tab)} onDoubleClick={() => beginRename(tab)} onContextMenu={event => {
                  event.preventDefault(); event.stopPropagation(); dismissPreview(); onActivate(tab); setNewMenu(false);
                  setContextMenu({ tab, anchor: event.currentTarget, x: event.clientX, y: event.clientY });
                }} onKeyDown={event => { if (event.target === event.currentTarget && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); onActivate(tab); } }}>
                <WorkspaceTabIcon kind={tab.kind} path={tab.targetId} terminal={tab.terminal} automationKind={tab.automationKind} />
                {rename?.tabId === tab.id ? <input className="project-aow-tab-rename-input" value={rename.draft}
                  maxLength={tab.kind === 'terminal' || tab.kind === 'agent' ? 128 : 255} autoFocus
                  aria-label={tab.kind === 'agent' ? 'Agent 名称' : tab.kind === 'terminal' ? 'Terminal 名称' : '文件名'}
                  onFocus={event => event.currentTarget.select()} onChange={event => setRename({ ...rename, draft: event.target.value })}
                  onClick={event => event.stopPropagation()} onDoubleClick={event => event.stopPropagation()}
                  onPointerDown={event => event.stopPropagation()} onContextMenu={event => event.stopPropagation()} onBlur={finishRename}
                  onKeyDown={event => { event.stopPropagation(); if (event.key === 'Enter') event.currentTarget.blur();
                    if (event.key === 'Escape') { cancelRename.current = true; event.currentTarget.blur(); } }} /> : <span>{tab.label}</span>}
                {tab.terminal && tab.terminal.agentCount > 1 && tab.label !== `${tab.terminal.agentCount} agents`
                  ? <small className="terminal-tab-agent-count" aria-label={`${tab.terminal.agentCount} agents`}>{tab.terminal.agentCount}</small> : null}
                {rename?.tabId !== tab.id ? <button className="project-aow-tab-close" title={`关闭 ${tab.label}`} aria-label={`关闭 ${tab.label}`}
                  onClick={event => { event.stopPropagation(); run(() => onCloseTab(tab)); }}><X /></button> : null}
              </div>) : null}
            </div>
          </div>;
        })}
      </div>
      <div ref={newMenuRef} className="project-aow-new" onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setNewMenu(false); }}>
        <button className="project-aow-new-trigger" title={newLabel} aria-label={newLabel} aria-expanded={newMenu} onClick={() => setNewMenu(value => !value)}><Plus /></button>
        {newMenu ? <WorkspaceNewMenu agents={agents} onOpenFile={onOpenFile} onOpenBrowser={onOpenBrowser}
          onCreateNote={onCreateNote} onCreateTerminal={onCreateTerminal} onSelect={() => setNewMenu(false)} /> : null}
      </div>
      {controls}
    </nav>
    {visible && preview && summary && !rename ? <TerminalTabPreview id={previewId} summary={summary}
      left={preview.left} top={preview.top} onDismiss={dismissPreview} /> : null}
    {contextMenu && tabs.some(tab => tab.id === contextMenu.tab.id) ? createPortal(<CenterTabContextMenu state={contextMenu} destination={destination(contextMenu.tab)}
      onOpenElsewhere={() => onOpenElsewhere(contextMenu.tab)} onClose={dismissMenu} onRename={() => beginRename(contextMenu.tab)}
      hasOtherTabs={tabs.some(tab => tab.id !== contextMenu.tab.id && workspaceTabGroup(tab) === workspaceTabGroup(contextMenu.tab))}
      onCloseTab={() => run(() => onCloseTab(contextMenu.tab))} onCloseOthers={() => closeGroup(contextMenu.tab, true)}
      onCloseAll={() => closeGroup(contextMenu.tab, false)} />, document.body) : null}
  </>;
}
