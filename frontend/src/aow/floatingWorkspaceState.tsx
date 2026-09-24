import { ListRowMenuContext } from '../components/ListRowMenuContext';
import { appLocalStorage } from '../lib/basePath';
import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import type { TerminalTab } from '../features/terminals/types';
import type { AowAgent } from '../features/agents/types';
import { isWorkspaceTabKind, type WorkspaceTab, type TabRevealRequest } from './WorkspaceTabs';
import { WorkspaceMenuLabel } from './WorkspaceMenuLabel';

export interface FloatingTab extends WorkspaceTab { workspace: string }
export interface HostedTab extends FloatingTab { host: string }
interface WorkspaceHost { portal: HTMLDivElement | null; activeId?: string; visible: boolean }
export interface WorkspaceCommands {
  tabs: WorkspaceTab[];
  reveal: (id: string) => void;
  create: (kind: 'markdown' | 'text' | 'terminal' | 'files', agent?: AowAgent) => void;
  close: (id: string) => void;
  // Closing a group dismisses its views; terminal instances keep running.
  closeMany: (ids: string[]) => Promise<void>;
  canRename: (id: string) => boolean;
  rename: (id: string, name: string) => Promise<void>;
}
const storageKey = 'aow-floating-tabs-v1';
const hostedStorageKey = 'aow-hosted-terminals-v1';
export function readStored<T>(key: string, fallback: T): T {
  try { return JSON.parse(appLocalStorage.getItem(key) ?? 'null') ?? fallback; } catch { return fallback; }
}
export function persist(key: string, value: unknown) {
  try { appLocalStorage.setItem(key, JSON.stringify(value)); } catch { /* The current session remains usable without storage. */ }
}
let floatingOpenDepth = 0;
export const openingInFloatingWorkspace = () => floatingOpenDepth > 0;
export function withFloatingOpen<T>(action: () => T): T {
  floatingOpenDepth += 1;
  try { return action(); } finally { floatingOpenDepth -= 1; }
}
function initialTabs(): FloatingTab[] {
  const value = readStored<unknown>(storageKey, []);
  return Array.isArray(value) ? value.filter((tab): tab is FloatingTab => tab && typeof tab.workspace === 'string'
    && typeof tab.id === 'string' && typeof tab.label === 'string' && isWorkspaceTabKind(tab.kind))
    .map(tab => ({ ...tab, targetId: tab.targetId ?? tab.id.slice(tab.id.indexOf(':') + 1),
      terminal: Array.isArray(tab.terminal?.panes) ? tab.terminal : undefined })) : [];
}
const tabKey = (tab: Pick<FloatingTab, 'workspace' | 'id'>) => JSON.stringify([tab.workspace, tab.id]);
function useFloatingController() {
  const [tabs, setTabs] = useState(initialTabs);
  const [hostedTabs, setHostedTabs] = useState<HostedTab[]>(() => {
    const saved = readStored<unknown>(hostedStorageKey, []);
    return Array.isArray(saved) ? saved.filter((tab): tab is HostedTab => tab
      && typeof tab.workspace === 'string' && typeof tab.host === 'string' && tab.host !== tab.workspace
      && typeof tab.id === 'string' && tab.id.startsWith('terminal:') && typeof tab.targetId === 'string'
      && typeof tab.label === 'string' && (tab.kind === 'terminal' || tab.kind === 'agent')
      && !tabs.some(item => tabKey(item) === tabKey(tab))) : [];
  });
  const [hosts, setHosts] = useState<Record<string, WorkspaceHost>>({});
  const [terminalRequests, setTerminalRequests] = useState<TerminalTab[]>([]);
  const requestTerminal = useCallback((tab: TerminalTab) => {
    setTerminalRequests(items => [...items.filter(item => item.id !== tab.id), tab]);
  }, []);
  const acknowledgeTerminal = useCallback((tab: TerminalTab) => {
    setTerminalRequests(items => items.filter(item => item !== tab));
  }, []);
  const cancelTerminalRequest = useCallback((id: string) => {
    setTerminalRequests(items => items.some(tab => tab.id === id) ? items.filter(tab => tab.id !== id) : items);
  }, []);
  const publishHost = useCallback((workspace: string, host?: WorkspaceHost) => {
    setHosts(items => {
      const previous = items[workspace];
      if (previous?.portal === host?.portal && previous?.activeId === host?.activeId && previous?.visible === host?.visible) return items;
      const next = { ...items };
      if (host) next[workspace] = host;
      else delete next[workspace];
      return next;
    });
  }, []);
  const removeHosted = useCallback((workspace: string, id: string) => {
    setHostedTabs(items => items.some(tab => tab.workspace === workspace && tab.id === id)
      ? items.filter(tab => tab.workspace !== workspace || tab.id !== id) : items);
  }, []);
  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const contains = useCallback((workspace: string, id: string) => tabsRef.current.some(tab => tab.workspace === workspace && tab.id === id), []);
  const [tabRevealRequest, setTabRevealRequest] = useState<TabRevealRequest>();
  const [activeKey, setActiveKey] = useState(() => readStored<string>('aow-floating-active', ''));
  const [open, setOpen] = useState(() => readStored<boolean>('aow-floating-open', false));
  const [pinned, setPinned] = useState(() => readStored<boolean>('aow-floating-pinned', true) !== false);
  const [autoHidden, setAutoHidden] = useState(false);
  const [showRequest, setShowRequest] = useState(0);
  const visible = open && !autoHidden;
  const [portal, setPortal] = useState<HTMLDivElement | null>(null);
  const [sidebar, setSidebar] = useState<HTMLDivElement | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const sources = useRef(new Map<string, WorkspaceCommands>());
  const returnFocus = useRef<HTMLElement | null>(null);
  const [globalRoot, setGlobalRoot] = useState('');
  const [notesMoves, setNotesMoves] = useState<{ from: string; to: string }[]>([]);
  const show = useCallback(() => {
    if (document.activeElement instanceof HTMLElement && !document.activeElement.closest('[data-floating-workspace]')) returnFocus.current = document.activeElement;
    setAutoHidden(false);
    setShowRequest(value => value + 1);
    setOpen(true);
  }, []);
  const minimize = useCallback(() => { setOpen(false); setAutoHidden(false); returnFocus.current?.focus({ preventScroll: true }); }, []);
  const togglePinned = useCallback(() => { setPinned(value => !value); setAutoHidden(false); }, []);
  const add = useCallback((workspace: string, id: string, initial?: WorkspaceTab) => {
    removeHosted(workspace, id);
    const descriptor = sources.current.get(workspace)?.tabs.find(tab => tab.id === id) ?? initial;
    const tab: FloatingTab = { workspace, ...(descriptor ?? { id, label: '正在打开…', kind: 'file', pending: true, targetId: id.slice(id.indexOf(':') + 1) }) };
    setTabs(items => items.some(item => tabKey(item) === tabKey(tab)) ? items : [...items, tab]);
    setActiveKey(tabKey(tab));
    setTabRevealRequest({ tabId: tabKey(tab) });
    show();
  }, [show, removeHosted]);
  const hostTerminal = useCallback((workspace: string, host: string, descriptor: WorkspaceTab) => {
    setTabs(items => items.filter(tab => tab.workspace !== workspace || tab.id !== descriptor.id));
    const tab = { ...(sources.current.get(workspace)?.tabs.find(tab => tab.id === descriptor.id) ?? descriptor), workspace, host };
    setHostedTabs(items => items.some(item => tabKey(item) === tabKey(tab))
      ? items.map(item => tabKey(item) === tabKey(tab) ? tab : item) : [...items, tab]);
  }, []);
  const restore = useCallback((workspace: string, restored: WorkspaceCommands['tabs']) => {
    setTabs(items => {
      const missing = restored.filter(tab => !items.some(item => item.workspace === workspace && item.id === tab.id));
      return missing.length ? [...items, ...missing.map(tab => ({ workspace, ...tab }))] : items;
    });
  }, []);
  const remove = useCallback((workspace: string, id: string) => {
    setTabs(items => items.filter(tab => tab.workspace !== workspace || tab.id !== id));
  }, []);
  const replaceDocument = useCallback((workspace: string, oldId: string, newId: string) => {
    const before = { workspace, id: `document:${oldId}` };
    const after = { workspace, id: `document:${newId}` };
    setTabs(items => items.map(tab => tabKey(tab) === tabKey(before) ? { ...tab, id: after.id } : tab)
      .filter((tab, index, all) => all.findIndex(item => tabKey(item) === tabKey(tab)) === index));
    setActiveKey(key => key === tabKey(before) ? tabKey(after) : key);
  }, []);
  const retainWorkspaces = useCallback((paths: string[]) => {
    setTabs(items => items.filter(tab => paths.includes(tab.workspace)));
    setHostedTabs(items => items.filter(tab => paths.includes(tab.workspace) && paths.includes(tab.host)));
    setTerminalRequests(items => items.filter(tab => paths.includes(tab.workspace_root)));
  }, []);
  const publish = useCallback((workspace: string, commands: WorkspaceCommands) => {
    sources.current.set(workspace, commands);
    const update = <T extends FloatingTab,>(items: T[]): T[] => {
      let changed = false;
      const next = items.map(item => {
        const tab = item.workspace === workspace ? commands.tabs.find(tab => tab.id === item.id) : undefined;
        if (!tab || !item.pending && tab.label === item.label && tab.kind === item.kind && tab.renameable === item.renameable
          && tab.targetId === item.targetId && tab.dirty === item.dirty && JSON.stringify(tab.terminal) === JSON.stringify(item.terminal)) return item;
        changed = true;
        return { ...item, ...tab, pending: false };
      });
      return changed ? next : items;
    };
    setTabs(update);
    setHostedTabs(update);
  }, []);
  useEffect(() => persist(hostedStorageKey, hostedTabs), [hostedTabs]);
  useEffect(() => {
    if (!tabs.some(tab => tabKey(tab) === activeKey)) setActiveKey(tabs.length ? tabKey(tabs[tabs.length - 1]) : '');
    persist(storageKey, tabs);
  }, [tabs, activeKey]);
  useEffect(() => persist('aow-floating-active', activeKey), [activeKey]);
  useEffect(() => persist('aow-floating-open', open), [open]);
  useEffect(() => persist('aow-floating-pinned', pinned), [pinned]);
  const active = tabs.find(tab => tabKey(tab) === activeKey);
  const select = (tab: FloatingTab) => { setActiveKey(tabKey(tab)); show(); };
  const close = (tab: FloatingTab) => {
    const source = sources.current.get(tab.workspace);
    if (source) source.close(tab.id);
    else remove(tab.workspace, tab.id);
  };
  return { tabs, tabRevealRequest, contains, active, open, visible, pinned, togglePinned, autoHidden, setAutoHidden, showRequest, portal, setPortal, sidebar, setSidebar, sidebarOpen, setSidebarOpen,
    globalRoot, setGlobalRoot, notesMoves, setNotesMoves, sources, show, minimize, add, restore, remove, replaceDocument, retainWorkspaces, publish, select, close,
    hostedTabs, hosts, publishHost, hostTerminal, removeHosted, terminalRequests, requestTerminal, acknowledgeTerminal, cancelTerminalRequest };
}
const FloatingContext = createContext<ReturnType<typeof useFloatingController> | null>(null);
export function FloatingWorkspaceProvider({ children }: { children: ReactNode }) {
  const value = useFloatingController();
  return <FloatingContext.Provider value={value}>{children}</FloatingContext.Provider>;
}
export function useFloatingWorkspace() {
  const value = useContext(FloatingContext);
  if (!value) throw new Error('FloatingWorkspaceProvider is missing');
  return value;
}

export function FloatingOpenMenu({ children }: { children: ReactNode }) {
  const [target, setTarget] = useState<{ element: HTMLElement; x: number; y: number }>();
  const menu = useRef<HTMLDivElement>(null);
  const open = useCallback((element: HTMLElement, x: number, y: number) => {
    setTarget({ element, x, y });
  }, []);
  useLayoutEffect(() => {
    if (!target || !menu.current) return;
    const bounds = menu.current.getBoundingClientRect();
    const x = Math.max(4, Math.min(target.x, window.innerWidth - bounds.width - 4));
    const y = Math.max(4, Math.min(target.y, window.innerHeight - bounds.height - 4));
    if (x !== target.x || y !== target.y) setTarget({ ...target, x, y });
  }, [target]);
  useEffect(() => {
    if (!target) return;
    const close = () => setTarget(undefined);
    window.addEventListener('pointerdown', close);
    window.addEventListener('resize', close);
    window.addEventListener('scroll', close, true);
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    window.addEventListener('keydown', key);
    return () => { window.removeEventListener('pointerdown', close); window.removeEventListener('resize', close);
      window.removeEventListener('scroll', close, true); window.removeEventListener('keydown', key); };
  }, [target]);
  return <ListRowMenuContext.Provider value={{ target: target?.element, open }}><div className="workspace-open-boundary" onContextMenuCapture={event => {
    const element = (event.target as HTMLElement).closest<HTMLElement>('[data-workspace-open]');
    if (!element) return;
    event.preventDefault(); event.stopPropagation();
    const bounds = element.getBoundingClientRect();
    open(element, event.clientX || bounds.x, event.clientY || bounds.bottom);
  }}>{children}{target ? <div ref={menu} className="project-aow-context-menu" role="menu" aria-label="选择打开位置"
    style={{ left: target.x, top: target.y }} onPointerDown={event => event.stopPropagation()} onMouseDown={event => event.preventDefault()}>
    <button role="menuitem" onClick={() => { target.element.click(); setTarget(undefined); }}><WorkspaceMenuLabel action="open" /></button>
    <button role="menuitem" onClick={() => { withFloatingOpen(() => target.element.click()); setTarget(undefined); }}><WorkspaceMenuLabel action="floating" /></button>
  </div> : null}</div></ListRowMenuContext.Provider>;
}
