import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { LoaderCircle, SquareTerminal } from 'lucide-react';
import { TerminalWorkspace } from '../features/terminals/TerminalWorkspace';
import { WorkspaceMarkdownProvider } from '../features/files/WorkspaceMarkdown';
import { terminalApi } from '../features/terminals/terminalApi';
import { terminalStore } from '../features/terminals/terminalStore';
import { useTerminalState } from '../features/terminals/useTerminalState';
import type { TerminalTab } from '../features/terminals/types';
import { persist, readStored } from './floatingWorkspaceState';
import type { AowProject, AowWorktree } from './types';
import './project-terminal-dock.css';

interface DockState { terminalId?: string; open: boolean; height: number }
interface DockContext {
  terminalId?: string;
  tab?: TerminalTab;
  open: boolean;
  visible: boolean;
  height: number;
  publishHost: (host: HTMLDivElement, active: boolean) => void;
  resize: (height: number) => void;
  show: () => void;
  owns: (tab: TerminalTab) => boolean;
}
const Context = createContext<DockContext | null>(null);
const storageKey = (id: string) => `aow-worktree-terminal-dock:${id}`;
const defaultHeight = 280;
const clampHeight = (height: number) => Math.max(120, Math.min(800, height));
function readState(key: string): DockState {
  const saved = readStored<Partial<DockState>>(key, {});
  return { terminalId: typeof saved.terminalId === 'string' ? saved.terminalId : undefined,
    open: saved.open === true, height: typeof saved.height === 'number' && Number.isFinite(saved.height) ? clampHeight(saved.height) : defaultHeight };
}

function readLegacyState(projectId: string): DockState {
  return readState(`aow-project-terminal-dock:${projectId}`);
}

/** One renderer per worktree. Moving its stable portal container preserves xterm and its connection. */
export function ProjectTerminalDockProvider({ project, worktree, controls, onShow, children }: {
  project: AowProject; worktree: AowWorktree; controls: HTMLElement | null; onShow: () => void; children: ReactNode;
}) {
  const [state, setState] = useState(() => readStored<Partial<DockState> | null>(storageKey(worktree.path), null) ? readState(storageKey(worktree.path))
    : worktree.is_main ? readLegacyState(project.id) : readState(storageKey(worktree.path)));
  const snapshot = useTerminalState(true);
  const [host, setHost] = useState<HTMLDivElement | null>(null);
  const [container] = useState(() => { const node = document.createElement('div'); node.className = 'project-terminal-dock-renderer'; return node; });
  const [creating, setCreating] = useState(false);
  const creatingRef = useRef(false);
  const [error, setError] = useState('');
  const [focusRequest, setFocusRequest] = useState(0);
  const mounted = useRef(true);
  const attemptedMigration = useRef<string | undefined>(undefined);
  const root = worktree.path;
  const serverTab = snapshot.tabs?.find(tab => tab.dock_project_id === project.id && tab.workspace_root === root);
  const legacyTab = !serverTab ? snapshot.tabs?.find(tab => tab.id === state.terminalId
    && !tab.dock_project_id && tab.workspace_root === root) : undefined;
  const tab = serverTab ?? legacyTab;
  const visible = !!host && state.open;
  const publishHost = useCallback((element: HTMLDivElement, active: boolean) => {
    setHost(current => active ? element : current === element ? null : current);
  }, []);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => { persist(storageKey(root), state); }, [root, state]);
  useLayoutEffect(() => { if (host) host.appendChild(container); }, [container, host]);
  useEffect(() => () => container.remove(), [container]);

  // Server ownership survives browser storage clearing and other clients creating the dock.
  useEffect(() => {
    if (serverTab && state.terminalId !== serverTab.id) {
      setState(current => ({ ...current, terminalId: serverTab.id }));
      return;
    }
    // Only an authoritative inventory may invalidate a saved association. Hiding never deletes a PTY.
    if (!state.terminalId || tab || creating || !snapshot.tabs || snapshot.error || snapshot.status === 'denied') return;
    setState(current => ({ ...current, terminalId: undefined, open: false }));
  }, [state.terminalId, serverTab, tab, creating, snapshot.tabs, snapshot.error, snapshot.status]);

  const ensureDock = useCallback(async (legacyId?: string) => {
    if (creatingRef.current) return;
    setError('');
    creatingRef.current = true;
    setCreating(true);
    const session = terminalStore.session();
    try {
      const created = await terminalApi.projectDock(project.id, root, legacyId);
      if (!mounted.current || !terminalStore.isCurrentSession(session)) return;
      setState(current => ({ ...current, terminalId: created.id }));
      terminalStore.replace(created);
    } catch (reason) {
      if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      creatingRef.current = false;
      if (mounted.current) setCreating(false);
    }
  }, [project.id, root]);

  // Upgrade only an explicitly saved, still-existing legacy instance; never infer ownership from its name.
  useEffect(() => {
    if (!legacyTab || creating || snapshot.error || snapshot.status === 'denied'
      || attemptedMigration.current === legacyTab.id) return;
    attemptedMigration.current = legacyTab.id;
    void ensureDock(legacyTab.id);
  }, [legacyTab, creating, snapshot.error, snapshot.status, ensureDock]);

  const show = async () => {
    if (creatingRef.current) return;
    onShow();
    setError('');
    setState(current => ({ ...current, open: true }));
    setFocusRequest(value => value + 1);
    if (serverTab) return;
    if (!snapshot.tabs || snapshot.error || snapshot.status === 'denied') {
      setError(snapshot.error || '正在确认终端实例，请稍后重试。');
      return;
    }
    await ensureDock(legacyTab?.id);
  };
  const close = () => setState(current => ({ ...current, open: false }));
  const owns = useCallback((candidate: TerminalTab) => (candidate.dock_project_id === project.id && candidate.workspace_root === root)
    || !serverTab && !candidate.dock_project_id && candidate.id === state.terminalId && candidate.workspace_root === root,
  [project.id, serverTab, state.terminalId, root]);
  return <Context.Provider value={{ terminalId: tab?.id, tab, open: state.open, visible, height: state.height,
    publishHost, resize: height => setState(current => ({ ...current, height: clampHeight(height) })), show: () => { void show(); }, owns }}>
    {children}
    {host && controls ? createPortal(<button type="button" className="project-terminal-dock-toggle" title="Worktree 终端" aria-label="Worktree 终端"
      aria-expanded={visible} disabled={creating} onClick={() => { if (visible) close(); else void show(); }}>
      {creating ? <LoaderCircle size={13} className="spinning" /> : <SquareTerminal size={13} />}
    </button>, controls) : null}
    {createPortal(<WorkspaceMarkdownProvider workspace={worktree.id} directory={root}>
      {error && <div className="project-terminal-dock-error" role="alert">{error}<button onClick={() => void show()}>重试</button></div>}
      {tab ? <TerminalWorkspace key={tab.id} visible={visible} autoFocus={false} focusRequest={focusRequest} tab={tab} loading={false}
        detectedAgents={snapshot.metadata.agents} terminalTitles={snapshot.metadata.titles} agentProcesses={snapshot.metadata.processes} terminalActivity={snapshot.metadata.activity}
        onTabChange={updated => terminalStore.replace(updated)} onTabClosed={id => { terminalStore.remove(id); setState(current => ({ ...current, terminalId: undefined, open: false })); }}
        onPaneStatus={(...args) => terminalStore.updatePaneStatus(...args)} onReload={() => { void terminalStore.refresh(); }} />
        : creating ? <div className="project-terminal-dock-loading"><LoaderCircle className="spinning" size={16} />正在创建 Worktree 终端…</div> : null}
    </WorkspaceMarkdownProvider>, container)}
  </Context.Provider>;
}

export function useProjectTerminalDock() {
  const value = useContext(Context);
  if (!value) throw new Error('Project terminal dock provider is missing');
  return value;
}

/** The host takes space only in the selected non-terminal page; its renderer lives in the provider. */
export function ProjectTerminalDockHost({ active }: { active: boolean }) {
  const dock = useProjectTerminalDock();
  const [element, setElement] = useState<HTMLDivElement | null>(null);
  const drag = useRef<{ y: number; height: number } | undefined>(undefined);
  const { publishHost } = dock;
  useLayoutEffect(() => {
    if (!element) return;
    publishHost(element, active);
    return () => publishHost(element, false);
  }, [element, active, publishHost]);
  const shown = active && dock.open;
  return <section className="project-terminal-dock" aria-label="Worktree 终端" hidden={!shown} style={{ height: dock.height }}>
    <div className="project-terminal-dock-resize" role="separator" aria-label="调整 Worktree 终端高度" aria-orientation="horizontal"
      tabIndex={0} aria-valuenow={Math.round(dock.height)} aria-valuemin={120} aria-valuemax={800}
      onPointerDown={event => { if (event.button !== 0) return; event.preventDefault(); drag.current = { y: event.clientY, height: event.currentTarget.parentElement!.clientHeight }; event.currentTarget.setPointerCapture(event.pointerId); }}
      onPointerMove={event => { if (drag.current) dock.resize(drag.current.height + drag.current.y - event.clientY); }}
      onPointerUp={event => { drag.current = undefined; event.currentTarget.releasePointerCapture(event.pointerId); }}
      onLostPointerCapture={() => { drag.current = undefined; }}
      onKeyDown={event => { if (event.key === 'ArrowUp' || event.key === 'ArrowDown') { event.preventDefault(); dock.resize(dock.height + (event.key === 'ArrowUp' ? 20 : -20)); } }} />
    <div ref={setElement} className="project-terminal-dock-host" />
  </section>;
}
