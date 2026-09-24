import { useEffect, useRef, useState } from 'react';
import { NotebookPen, Files, Maximize2, Minimize2, Minus, PanelsTopLeft, Pin, PinOff, SquareTerminal, X, PanelRight } from 'lucide-react';
import { persist, readStored, useFloatingWorkspace, type FloatingTab } from './floatingWorkspaceState';
import type { AowAgent } from '../features/agents/types';
import { WorkspaceTabs, type WorkspaceTab } from './WorkspaceTabs';
import { useFloatingAutoHide } from './useFloatingAutoHide';
import './floating-workspace.css';

interface Bounds { left: number; top: number; width: number; height: number }
const defaultBounds = () => ({ left: Math.max(12, (window.innerWidth - 1000) / 2), top: 65, width: Math.min(1100, window.innerWidth - 24), height: Math.min(720, window.innerHeight - 100) });
function clamp(bounds: Bounds): Bounds {
  const width = Math.min(Math.max(420, bounds.width), window.innerWidth - 16);
  const height = Math.min(Math.max(280, bounds.height), window.innerHeight - 16);
  return { width, height, left: Math.max(8, Math.min(bounds.left, window.innerWidth - width - 8)), top: Math.max(8, Math.min(bounds.top, window.innerHeight - height - 8)) };
}
export function FloatingWorkspace({ agents }: { agents: AowAgent[] }) {
  const floating = useFloatingWorkspace();
  const [bounds, setBounds] = useState(() => {
    const stored = readStored<Bounds>('aow-floating-bounds', defaultBounds());
    return clamp(stored && [stored.left, stored.top, stored.width, stored.height].every(Number.isFinite) ? stored : defaultBounds());
  });
  const [maximized, setMaximized] = useState(() => readStored('aow-floating-maximized', false));
  const [operationError, setOperationError] = useState('');
  const tabKey = (tab: FloatingTab) => JSON.stringify([tab.workspace, tab.id]);
  const tabs = floating.tabs.map(tab => ({ ...tab, id: tabKey(tab), title: `${tab.label}\n${tab.workspace}` }));
  const sourceTab = (tab: WorkspaceTab) => floating.tabs.find(item => tabKey(item) === tab.id)!;
  const closeTabs = async (targets: WorkspaceTab[]) => {
    const byWorkspace = new Map<string, string[]>();
    for (const target of targets) {
      const tab = sourceTab(target);
      const ids = byWorkspace.get(tab.workspace) ?? [];
      ids.push(tab.id);
      byWorkspace.set(tab.workspace, ids);
    }
    for (const [workspace, ids] of byWorkspace) {
      const source = floating.sources.current.get(workspace);
      if (source) await source.closeMany(ids);
      else for (const id of ids) floating.remove(workspace, id);
    }
  };
  const drag = useRef<{ x: number; y: number; bounds: Bounds; resize: boolean } | undefined>(undefined);
  const launcherDrag = useRef<{ x: number; y: number; left: number; top: number; moved: boolean } | undefined>(undefined);
  const [launcher, setLauncher] = useState(() => {
    const fallback = { left: window.innerWidth - 70, top: window.innerHeight - 90 };
    const stored = readStored('aow-floating-launcher', fallback);
    return stored && Number.isFinite(stored.left) && Number.isFinite(stored.top) ? stored : fallback;
  });
  const panelRef = useRef<HTMLDivElement>(null);
  useFloatingAutoHide(panelRef, floating);
  useEffect(() => { persist('aow-floating-bounds', bounds); }, [bounds]);
  useEffect(() => { persist('aow-floating-maximized', maximized); }, [maximized]);
  useEffect(() => { persist('aow-floating-launcher', launcher); }, [launcher]);
  useEffect(() => {
    const resize = () => { setBounds(value => clamp(value)); setLauncher(value => ({ left: Math.max(8, Math.min(value.left, window.innerWidth - 54)), top: Math.max(8, Math.min(value.top, window.innerHeight - 54)) })); };
    resize(); window.addEventListener('resize', resize);
    return () => window.removeEventListener('resize', resize);
  }, []);
  useEffect(() => {
    if (floating.visible) panelRef.current?.focus({ preventScroll: true });
    else if (document.activeElement instanceof HTMLElement && panelRef.current?.contains(document.activeElement)) document.activeElement.blur();
  }, [floating.visible]);
  const create = (kind: 'markdown' | 'text' | 'terminal' | 'files', agent?: AowAgent) => {
    floating.sources.current.get(floating.globalRoot)?.create(kind, agent);
  };
  const actions = <>
    <button disabled={!floating.globalRoot} onClick={() => create('markdown')}><NotebookPen />新建 Markdown</button>
    <button disabled={!floating.globalRoot} onClick={() => create('terminal')}><SquareTerminal />新建 Terminal</button>
    <button disabled={!floating.globalRoot} onClick={() => create('files')}><Files />打开系统文件浏览器</button>
  </>;
  return <>
    <button className="floating-workspace-trigger" style={launcher} title="浮动工作区" aria-label="浮动工作区" aria-expanded={floating.visible}
      onPointerDown={event => { if (event.button) return; launcherDrag.current = { x: event.clientX, y: event.clientY, ...launcher, moved: false }; event.currentTarget.setPointerCapture(event.pointerId); }}
      onPointerMove={event => { const start = launcherDrag.current; if (!start) return;
        if (Math.hypot(event.clientX - start.x, event.clientY - start.y) > 4) start.moved = true;
        if (start.moved) setLauncher({ left: Math.max(8, Math.min(window.innerWidth - 54, start.left + event.clientX - start.x)), top: Math.max(8, Math.min(window.innerHeight - 54, start.top + event.clientY - start.y)) }); }}
      onPointerUp={() => { if (!launcherDrag.current?.moved) floating.visible ? floating.minimize() : floating.show(); launcherDrag.current = undefined; }}
      onPointerCancel={() => { launcherDrag.current = undefined; }}
      onClick={event => { if (event.detail === 0) floating.visible ? floating.minimize() : floating.show(); }}><PanelsTopLeft /></button>
    <div ref={panelRef} data-floating-workspace className={`floating-workspace${maximized ? ' maximized' : ''}`} role="dialog" aria-label="浮动工作区" aria-hidden={!floating.visible} inert={!floating.visible}
      tabIndex={-1} style={{ ...(maximized ? { left: 8, top: 8, width: window.innerWidth - 16, height: window.innerHeight - 16 } : bounds), visibility: floating.visible ? 'visible' : 'hidden', pointerEvents: floating.visible ? undefined : 'none' }}>
      <WorkspaceTabs workspaceKey="floating" revealRequest={floating.tabRevealRequest} tabs={tabs} activeId={floating.active ? tabKey(floating.active) : undefined} visible={floating.visible} agents={agents}
        onActivate={tab => floating.select(sourceTab(tab))} onCloseTab={tab => floating.close(sourceTab(tab))} onCloseTabs={closeTabs}
        onCanRename={tab => {
          const source = sourceTab(tab);
          setOperationError('');
          return floating.sources.current.get(source.workspace)?.canRename(source.id) ?? false;
        }}
        onRename={async (tab, name) => {
          const source = sourceTab(tab);
          await floating.sources.current.get(source.workspace)?.rename(source.id, name);
        }} onError={setOperationError}
        destination={tab => sourceTab(tab).workspace !== floating.globalRoot ? 'restore' : undefined}
        onOpenElsewhere={tab => {
          const source = sourceTab(tab);
          floating.remove(source.workspace, source.id);
          floating.sources.current.get(source.workspace)?.reveal(source.id);
        }}
        onOpenBrowser={() => create('files')} onCreateNote={kind => create(kind === 'md' ? 'markdown' : 'text')}
        onCreateTerminal={agent => create('terminal', agent)} newLabel="浮动工作区新建" tabListLabel="浮动工作区标签"
        controls={<div className="floating-workspace-window-actions">
          <button title="全局项目面板" aria-label="全局项目面板" aria-pressed={floating.sidebarOpen} onClick={() => floating.setSidebarOpen(!floating.sidebarOpen)}><PanelRight /></button>
          <button className="floating-workspace-pin" title={floating.pinned ? '取消固定：移出后自动隐藏' : '固定显示浮动工作区'} aria-label="固定显示浮动工作区" aria-pressed={floating.pinned} onClick={floating.togglePinned}>{floating.pinned ? <Pin /> : <PinOff />}</button>
          <button title={maximized ? '还原浮动工作区' : '最大化浮动工作区'} aria-label={maximized ? '还原浮动工作区' : '最大化浮动工作区'} onClick={() => setMaximized(!maximized)}>{maximized ? <Minimize2 /> : <Maximize2 />}</button>
          <button title="最小化浮动工作区" aria-label="最小化浮动工作区" onClick={floating.minimize}><Minus /></button>
        </div>}
        headerProps={{
          className: 'floating-workspace-header',
          onDoubleClick: event => { if (!(event.target as HTMLElement).closest('button,[role=tab]')) setMaximized(value => !value); },
          onPointerDown: event => { if (maximized || event.button || (event.target as HTMLElement).closest('button,[role=tab]')) return;
            drag.current = { x: event.clientX, y: event.clientY, bounds, resize: false }; event.currentTarget.setPointerCapture(event.pointerId); },
          onPointerMove: event => { const start = drag.current; if (!start || start.resize) return; setBounds(clamp({ ...start.bounds, left: start.bounds.left + event.clientX - start.x, top: start.bounds.top + event.clientY - start.y })); },
          onPointerUp: () => { drag.current = undefined; }, onPointerCancel: () => { drag.current = undefined; },
        }}
      />
      {operationError ? <div className="project-aow-inline-error" role="alert">{operationError}<button aria-label="关闭错误提示" onClick={() => setOperationError('')}><X /></button></div> : null}
      <div className="floating-workspace-body">
        <div className="floating-workspace-content" ref={floating.setPortal}>
          {!floating.tabs.length ? <div className="floating-workspace-empty">{actions}{!floating.globalRoot ? <p>正在加载全局项目…</p> : null}</div> : null}
        </div>
        <div className="floating-workspace-sidebar" hidden={!floating.sidebarOpen} ref={floating.setSidebar} />
      </div>
      {!maximized ? <div className="floating-workspace-resize" role="separator" aria-label="调整浮动工作区大小"
        onPointerDown={event => { drag.current = { x: event.clientX, y: event.clientY, bounds, resize: true }; event.currentTarget.setPointerCapture(event.pointerId); }}
        onPointerMove={event => { const start = drag.current; if (!start?.resize) return; setBounds(clamp({ ...start.bounds, width: start.bounds.width + event.clientX - start.x, height: start.bounds.height + event.clientY - start.y })); }}
        onPointerUp={() => { drag.current = undefined; }} onPointerCancel={() => { drag.current = undefined; }} /> : null}
    </div>
  </>;
}
