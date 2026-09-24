import { useEffect, useRef, useState, type DragEvent as ReactDragEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { Columns2, GripVertical, LoaderCircle, Maximize2, Minimize2, Rows2, SquareTerminal, X } from 'lucide-react';
import { terminalApi } from './terminalApi';
import type { TerminalAgentProcess, TerminalLayout, TerminalPane, TerminalPaneStatus, TerminalSplitAxis, TerminalTab } from './types';
import { TerminalInputPane } from './TerminalInputPane';
import { TerminalAgentPane } from './TerminalAgentPane';
import { AgentIcon } from '../agents/AgentIcon';
import { terminalPaneAgent, terminalPaneTitle } from './terminalPresentation';
import { terminalPaneLifecycle } from './terminalState';
import { TerminalLifecycleDot } from './TerminalLifecycleDot';
import { useConfirmation } from '../../components/ConfirmationDialog';

interface Props {
  visible: boolean;
  tab?: TerminalTab;
  loading: boolean;
  detectedAgents: Record<string, string | null>;
  terminalTitles: Record<string, string>;
  agentProcesses?: Record<string, TerminalAgentProcess>;
  onTabChange: (tab: TerminalTab) => void;
  onTabClosed: (tabId: string) => void;
  onPaneStatus: (tabId: string, paneId: string, status: TerminalPaneStatus, exitCode?: number | null) => void;
  onReload: () => void;
}

function message(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

function paneIds(layout: TerminalLayout | null): string[] {
  if (!layout) return [];
  if (layout.type === 'pane') return [layout.pane_id];
  return [...paneIds(layout.first), ...paneIds(layout.second)];
}

function replaceRatio(layout: TerminalLayout, path: number[], ratio: number): TerminalLayout {
  if (!path.length) return layout.type === 'split' ? { ...layout, ratio } : layout;
  if (layout.type !== 'split') return layout;
  const [branch, ...rest] = path;
  return branch === 0
    ? { ...layout, first: replaceRatio(layout.first, rest, ratio) }
    : { ...layout, second: replaceRatio(layout.second, rest, ratio) };
}

type PaneDropDirection = 'left' | 'right' | 'top' | 'bottom';

interface PaneDropTarget {
  paneId: string;
  direction: PaneDropDirection;
}

const paneDropLabels: Record<PaneDropDirection, string> = {
  left: '移到左侧',
  right: '移到右侧',
  top: '移到上方',
  bottom: '移到下方',
};

function removePaneFromLayout(layout: TerminalLayout, paneId: string): TerminalLayout | null {
  if (layout.type === 'pane') return layout.pane_id === paneId ? null : layout;
  const first = removePaneFromLayout(layout.first, paneId);
  const second = removePaneFromLayout(layout.second, paneId);
  if (!first) return second;
  if (!second) return first;
  return first === layout.first && second === layout.second ? layout : { ...layout, first, second };
}

function insertPaneAtTarget(
  layout: TerminalLayout,
  sourcePaneId: string,
  targetPaneId: string,
  direction: PaneDropDirection,
): TerminalLayout {
  if (layout.type === 'pane') {
    if (layout.pane_id !== targetPaneId) return layout;
    const source: TerminalLayout = { type: 'pane', pane_id: sourcePaneId };
    const sourceFirst = direction === 'left' || direction === 'top';
    return {
      type: 'split',
      axis: direction === 'left' || direction === 'right' ? 'row' : 'column',
      ratio: 0.5,
      first: sourceFirst ? source : layout,
      second: sourceFirst ? layout : source,
    };
  }
  if (paneIds(layout.first).includes(targetPaneId)) {
    return { ...layout, first: insertPaneAtTarget(layout.first, sourcePaneId, targetPaneId, direction) };
  }
  return { ...layout, second: insertPaneAtTarget(layout.second, sourcePaneId, targetPaneId, direction) };
}

function movePaneInLayout(
  layout: TerminalLayout,
  sourcePaneId: string,
  targetPaneId: string,
  direction: PaneDropDirection,
): TerminalLayout | null {
  if (sourcePaneId === targetPaneId) return null;
  const ids = paneIds(layout);
  if (!ids.includes(sourcePaneId) || !ids.includes(targetPaneId)) return null;
  const remaining = removePaneFromLayout(layout, sourcePaneId);
  return remaining ? insertPaneAtTarget(remaining, sourcePaneId, targetPaneId, direction) : null;
}

function paneDropDirection(event: ReactDragEvent<HTMLElement>): PaneDropDirection {
  const bounds = event.currentTarget.getBoundingClientRect();
  const x = Math.min(1, Math.max(0, (event.clientX - bounds.left) / Math.max(bounds.width, 1)));
  const y = Math.min(1, Math.max(0, (event.clientY - bounds.top) / Math.max(bounds.height, 1)));
  const distances: Array<[PaneDropDirection, number]> = [
    ['left', x],
    ['right', 1 - x],
    ['top', y],
    ['bottom', 1 - y],
  ];
  return distances.reduce((nearest, candidate) => candidate[1] < nearest[1] ? candidate : nearest)[0];
}

function fallbackLayout(tab: TerminalTab): TerminalLayout | null {
  return tab.layout ?? (tab.panes[0] ? { type: 'pane', pane_id: tab.panes[0].id } : null);
}

type TerminalShortcutPlatform = 'macos' | 'windows' | 'linux';

function terminalShortcutPlatform(): TerminalShortcutPlatform {
  if (/Mac|iPhone|iPad|iPod/.test(navigator.userAgent)) return 'macos';
  if (/Windows/.test(navigator.userAgent)) return 'windows';
  return 'linux';
}

function splitShortcutAxis(event: KeyboardEvent, platform: TerminalShortcutPlatform): TerminalSplitAxis | undefined {
  const key = event.key.toLowerCase();
  if (platform === 'macos') {
    if (!event.metaKey || event.ctrlKey || event.altKey || key !== 'd') return undefined;
    return event.shiftKey ? 'column' : 'row';
  }
  if (platform === 'windows') {
    if (!event.altKey || !event.shiftKey || event.ctrlKey || event.metaKey) return undefined;
    if (key === '=' || key === '+') return 'row';
    return key === '-' ? 'column' : undefined;
  }
  if (!event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey) return undefined;
  if (key === 'e') return 'row';
  return key === 'o' ? 'column' : undefined;
}

function splitShortcutLabel(axis: TerminalSplitAxis) {
  const platform = terminalShortcutPlatform();
  if (platform === 'macos') return axis === 'row' ? '⌘D' : '⇧⌘D';
  if (platform === 'windows') return axis === 'row' ? 'Alt+Shift+=' : 'Alt+Shift+-';
  return axis === 'row' ? 'Ctrl+Shift+E' : 'Ctrl+Shift+O';
}

export function TerminalWorkspace({ visible, tab, loading, detectedAgents, terminalTitles, agentProcesses = {}, onTabChange, onTabClosed, onPaneStatus, onReload }: Props) {
  const { confirm, confirmationDialog } = useConfirmation();
  const [layout, setLayout] = useState<TerminalLayout | null>(() => tab ? fallbackLayout(tab) : null);
  const [activePaneId, setActivePaneId] = useState<string>();
  const [draggingPaneId, setDraggingPaneId] = useState<string>();
  const [paneDropTarget, setPaneDropTarget] = useState<PaneDropTarget>();
  const [maximizedPaneIds, setMaximizedPaneIds] = useState<Record<string, string>>({});
  const [error, setError] = useState('');
  const [operation, setOperation] = useState('');
  const layoutRef = useRef(layout);
  const draggingPaneIdRef = useRef<string | undefined>(undefined);
  const tabRef = useRef(tab);
  const revisionRef = useRef(tab?.revision);
  const saveTimerRef = useRef<number | undefined>(undefined);
  const savingRef = useRef(false);
  const pendingLayoutRef = useRef<TerminalLayout | undefined>(undefined);
  const flushRequestedRef = useRef(false);
  const lastSaveRef = useRef(0);
  const saveEpochRef = useRef(0);
  const terminalWorkspace = useRef<HTMLElement>(null);

  layoutRef.current = layout;
  tabRef.current = tab;
  const maximizedPaneId = tab ? maximizedPaneIds[tab.id] : undefined;

  const setMaximizedPaneId = (
    tabId: string,
    update: string | undefined | ((current: string | undefined) => string | undefined),
  ) => {
    setMaximizedPaneIds((current) => {
      const previous = current[tabId];
      const next = typeof update === 'function' ? update(previous) : update;
      if (next === previous) return current;
      if (next) return { ...current, [tabId]: next };
      if (!(tabId in current)) return current;
      const remaining = { ...current };
      delete remaining[tabId];
      return remaining;
    });
  };

  useEffect(() => {
    const next = tab ? fallbackLayout(tab) : null;
    setLayout(next);
    layoutRef.current = next;
    revisionRef.current = tab?.revision;
    const ids = paneIds(next);
    setActivePaneId((current) => current && ids.includes(current)
      ? current
      : maximizedPaneId && ids.includes(maximizedPaneId) ? maximizedPaneId : ids[0]);
    setDraggingPaneId(undefined);
    draggingPaneIdRef.current = undefined;
    setPaneDropTarget(undefined);
    setError('');
  }, [tab?.id, tab?.layout, tab?.revision]);

  useEffect(() => {
    saveEpochRef.current += 1;
    if (saveTimerRef.current !== undefined) {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = undefined;
    }
    pendingLayoutRef.current = undefined;
    flushRequestedRef.current = false;
    savingRef.current = false;
  }, [tab?.id]);

  useEffect(() => {
    if (maximizedPaneId && ((tab?.panes.length ?? 0) <= 1
      || !tab?.panes.some((pane) => pane.id === maximizedPaneId))) {
      const tabId = tab?.id;
      if (tabId) setMaximizedPaneId(tabId, undefined);
    }
  }, [maximizedPaneId, tab?.panes]);

  useEffect(() => () => {
    if (saveTimerRef.current !== undefined) window.clearTimeout(saveTimerRef.current);
  }, []);

  const runLayoutSave = async () => {
    if (savingRef.current || !pendingLayoutRef.current) return;
    if (saveTimerRef.current !== undefined) {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = undefined;
    }
    const currentTab = tabRef.current;
    const candidate = pendingLayoutRef.current;
    if (!currentTab) return;
    const saveEpoch = saveEpochRef.current;
    pendingLayoutRef.current = undefined;
    savingRef.current = true;
    lastSaveRef.current = Date.now();
    try {
      const updated = await terminalApi.updateLayout(currentTab.id, candidate, revisionRef.current);
      if (updated) {
        const stillActive = saveEpochRef.current === saveEpoch && tabRef.current?.id === currentTab.id;
        if (stillActive) revisionRef.current = updated.revision;
        onTabChange({ ...updated, layout: stillActive ? layoutRef.current ?? updated.layout : updated.layout });
      }
    } catch (reason) {
      if (saveEpochRef.current === saveEpoch) setError(`布局保存失败：${message(reason)}`);
    } finally {
      if (saveEpochRef.current !== saveEpoch) return;
      savingRef.current = false;
      if (pendingLayoutRef.current) {
        if (flushRequestedRef.current) {
          flushRequestedRef.current = false;
          void runLayoutSave();
        } else {
          const delay = Math.max(0, 250 - (Date.now() - lastSaveRef.current));
          saveTimerRef.current = window.setTimeout(() => void runLayoutSave(), delay);
        }
      } else {
        flushRequestedRef.current = false;
      }
    }
  };

  const queueLayoutSave = (next: TerminalLayout, flush = false) => {
    pendingLayoutRef.current = next;
    if (flush) flushRequestedRef.current = true;
    if (savingRef.current) return;
    if (flush) {
      void runLayoutSave();
      return;
    }
    if (saveTimerRef.current !== undefined) return;
    const delay = Math.max(0, 250 - (Date.now() - lastSaveRef.current));
    saveTimerRef.current = window.setTimeout(() => void runLayoutSave(), delay);
  };

  const resizeSplit = (event: ReactPointerEvent<HTMLDivElement>, axis: TerminalSplitAxis, path: number[]) => {
    const divider = event.currentTarget;
    const container = divider.parentElement;
    if (!container || !layoutRef.current) return;
    event.preventDefault();
    const pointerId = event.pointerId;
    divider.setPointerCapture(pointerId);
    document.body.classList.add(axis === 'row' ? 'resizing-terminal-row' : 'resizing-terminal-column');

    const update = (clientX: number, clientY: number) => {
      const bounds = container.getBoundingClientRect();
      const raw = axis === 'row'
        ? (clientX - bounds.left) / bounds.width
        : (clientY - bounds.top) / bounds.height;
      const ratio = Math.min(.9, Math.max(.1, raw));
      const current = layoutRef.current;
      if (!current) return;
      const next = replaceRatio(current, path, ratio);
      layoutRef.current = next;
      setLayout(next);
      queueLayoutSave(next);
    };
    const move = (moveEvent: PointerEvent) => update(moveEvent.clientX, moveEvent.clientY);
    const stop = (stopEvent: PointerEvent) => {
      update(stopEvent.clientX, stopEvent.clientY);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
      document.body.classList.remove('resizing-terminal-row', 'resizing-terminal-column');
      if (divider.hasPointerCapture(pointerId)) divider.releasePointerCapture(pointerId);
      if (layoutRef.current) queueLayoutSave(layoutRef.current, true);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop, { once: true });
    window.addEventListener('pointercancel', stop, { once: true });
  };

  const finishPaneDrag = () => {
    draggingPaneIdRef.current = undefined;
    setDraggingPaneId(undefined);
    setPaneDropTarget(undefined);
  };

  const startPaneDrag = (event: ReactDragEvent<HTMLElement>, paneId: string) => {
    if (operation || tab?.panes.length === 1 || (event.target as HTMLElement).closest('button')) {
      event.preventDefault();
      return;
    }
    setActivePaneId(paneId);
    draggingPaneIdRef.current = paneId;
    setDraggingPaneId(paneId);
    setPaneDropTarget(undefined);
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('application/x-aow-terminal-pane', paneId);
    event.dataTransfer.setData('text/plain', paneId);
  };

  const updatePaneDropTarget = (event: ReactDragEvent<HTMLElement>, targetPaneId: string) => {
    const sourcePaneId = draggingPaneIdRef.current
      || event.dataTransfer.getData('application/x-aow-terminal-pane');
    if (!sourcePaneId || sourcePaneId === targetPaneId || operation) {
      setPaneDropTarget((current) => current?.paneId === targetPaneId ? undefined : current);
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    const direction = paneDropDirection(event);
    setPaneDropTarget((current) => current?.paneId === targetPaneId && current.direction === direction
      ? current
      : { paneId: targetPaneId, direction });
  };

  const dropPane = (event: ReactDragEvent<HTMLElement>, targetPaneId: string) => {
    event.preventDefault();
    event.stopPropagation();
    const sourcePaneId = draggingPaneIdRef.current
      || event.dataTransfer.getData('application/x-aow-terminal-pane');
    const direction = paneDropDirection(event);
    const current = layoutRef.current;
    const next = current && sourcePaneId
      ? movePaneInLayout(current, sourcePaneId, targetPaneId, direction)
      : null;
    if (next) {
      layoutRef.current = next;
      setLayout(next);
      setActivePaneId(sourcePaneId);
      setError('');
      queueLayoutSave(next, true);
    }
    finishPaneDrag();
  };

  const split = async (pane: TerminalPane, axis: TerminalSplitAxis) => {
    if (!tab) return;
    setOperation(`split:${pane.id}`);
    setError('');
    try {
      const updated = await terminalApi.split(tab.id, pane.id, axis, {
        cwd: pane.cwd || undefined,
        shell: pane.kind === 'terminal' ? pane.shell || undefined : undefined,
      });
      if (updated) {
        onTabChange(updated);
        const ids = paneIds(updated.layout);
        setActivePaneId(ids.find((id) => !tab.panes.some((oldPane) => oldPane.id === id)) ?? pane.id);
      } else {
        onReload();
      }
    } catch (reason) {
      setError(`分屏失败：${message(reason)}`);
    } finally {
      setOperation('');
    }
  };

  useEffect(() => {
    const splitWithShortcut = (event: KeyboardEvent) => {
      if (
        event.repeat
        || !visible
        || operation
        || !terminalWorkspace.current?.contains(event.target as Node)
        || !(document.activeElement instanceof Element && document.activeElement.closest('.terminal-emulator'))
      ) return;

      const axis = splitShortcutAxis(event, terminalShortcutPlatform());
      const pane = activePaneId ? tab?.panes.find((item) => item.id === activePaneId) : undefined;
      if (!axis || !pane) return;

      event.preventDefault();
      void split(pane, axis);
    };

    window.addEventListener('keydown', splitWithShortcut, true);
    return () => window.removeEventListener('keydown', splitWithShortcut, true);
  }, [activePaneId, operation, tab, visible]);

  const togglePaneMaximized = (pane: TerminalPane) => {
    if (!tab || operation) return;
    setActivePaneId(pane.id);
    setDraggingPaneId(undefined);
    draggingPaneIdRef.current = undefined;
    setPaneDropTarget(undefined);
    setMaximizedPaneId(tab.id, (current) => current === pane.id ? undefined : pane.id);
  };

  const selectMaximizedPane = (paneId: string) => {
    if (!tab || !maximizedPaneId || paneId === maximizedPaneId) return;
    setActivePaneId(paneId);
    setMaximizedPaneId(tab.id, paneId);
  };

  const closePane = async (pane: TerminalPane) => {
    if (!tab) return;
    const closingLastPane = tab.panes.length === 1;
    if (!await confirm({
      title: closingLastPane ? '关闭终端？' : '关闭终端窗口？',
      description: closingLastPane
        ? `这是“${tab.name}”的最后一个窗口，关闭后也会关闭对应的终端标签页。`
        : '将关闭以下终端窗口。',
      items: [{ id: pane.id, label: terminalPaneTitle(pane, detectedAgents, terminalTitles), detail: pane.cwd, icon: <SquareTerminal /> }],
      warning: '此窗口中仍在运行的命令将被终止，此操作无法撤销。',
      confirmLabel: closingLastPane ? '关闭终端' : '关闭窗口',
      danger: true,
    })) return;
    if (tabRef.current?.id !== tab.id || !tabRef.current.panes.some((item) => item.id === pane.id)) return;
    setOperation(`close:${pane.id}`);
    setError('');
    try {
      const updated = await terminalApi.closePane(tab.id, pane.id);
      if (updated) {
        if (maximizedPaneId === pane.id) {
          const currentIds = paneIds(layoutRef.current);
          const closedIndex = currentIds.indexOf(pane.id);
          const remainingIds = paneIds(fallbackLayout(updated));
          const nextPaneId = remainingIds[Math.min(Math.max(closedIndex, 0), remainingIds.length - 1)];
          setMaximizedPaneId(tab.id, updated.panes.length > 1 ? nextPaneId : undefined);
          setActivePaneId(nextPaneId);
        }
        onTabChange(updated);
      } else {
        setMaximizedPaneId(tab.id, undefined);
        onTabClosed(tab.id);
      }
    } catch (reason) {
      setError(`关闭 pane 失败：${message(reason)}`);
    } finally {
      setOperation('');
    }
  };

  const markStatus = (paneId: string, status: TerminalPaneStatus, exitCode?: number | null) => {
    const currentTab = tabRef.current;
    if (currentTab) {
      onPaneStatus(currentTab.id, paneId, status, exitCode);
      onReload();
    }
  };

  const naturalPaneOrder = paneIds(layout).filter((id) => tab?.panes.some((pane) => pane.id === id));

  const renderLayout = (node: TerminalLayout, path: number[] = []): ReactNode => {
    if (node.type === 'pane') {
      const pane = tab?.panes.find((item) => item.id === node.pane_id);
      if (!pane) return <div className="terminal-pane-missing">Pane {node.pane_id} 不存在</div>;
      const agentId = terminalPaneAgent(pane, detectedAgents);
      const title = terminalPaneTitle(pane, detectedAgents, terminalTitles);
      const paneBusy = operation.endsWith(`:${pane.id}`);
      const paneMaximized = maximizedPaneId === pane.id;
      const paneObscured = Boolean(maximizedPaneId && !paneMaximized);
      const paneDraggable = !operation && !maximizedPaneId && tab!.panes.length > 1;
      const dropDirection = paneDropTarget?.paneId === pane.id ? paneDropTarget.direction : undefined;
      return (
        <section
          key={pane.id}
          className={[
            'terminal-pane',
            activePaneId === pane.id ? 'active' : '',
            paneMaximized ? 'maximized' : '',
            paneObscured ? 'pane-obscured' : '',
            draggingPaneId === pane.id ? 'pane-dragging' : '',
            dropDirection ? `pane-drop-target drop-${dropDirection}` : '',
          ].filter(Boolean).join(' ')}
          data-drop-label={dropDirection ? paneDropLabels[dropDirection] : undefined}
          aria-hidden={paneObscured || undefined}
          onPointerDown={() => setActivePaneId(pane.id)}
          onDragOver={(event) => updatePaneDropTarget(event, pane.id)}
          onDrop={(event) => dropPane(event, pane.id)}
        >
          <TerminalAgentPane tabId={tab!.id} paneId={pane.id} agentId={agentId}
            title={terminalTitles[pane.id] ?? ''} cwd={pane.cwd} process={agentProcesses[pane.id]}
            visible={visible && !paneObscured}
            header={sessionButton => <header
            className={`terminal-pane-header ${paneDraggable ? 'draggable' : ''}`}
            draggable={paneDraggable}
            onDragStart={(event) => startPaneDrag(event, pane.id)}
            onDragEnd={finishPaneDrag}
          >
            <GripVertical className="terminal-pane-drag-handle" aria-label="拖动 pane 调整布局" />
            <TerminalLifecycleDot className="terminal-pane-status" state={terminalPaneLifecycle(pane)} />
            {agentId ? <AgentIcon agentId={agentId} /> : <SquareTerminal />}
            <span className="terminal-pane-name" title={`${title}\n路径：${pane.cwd || '未知'}`}>
              {title}
            </span>
            <span className="terminal-pane-shell">{pane.shell}</span>
            {sessionButton}
            <button title={`向右分屏（${splitShortcutLabel('row')}）`} aria-label="向右分屏" disabled={paneBusy} onClick={() => void split(pane, 'row')}><Columns2 /></button>
            <button title={`向下分屏（${splitShortcutLabel('column')}）`} aria-label="向下分屏" disabled={paneBusy} onClick={() => void split(pane, 'column')}><Rows2 /></button>
            {tab!.panes.length > 1 ? (
              <button
                title={paneMaximized ? '恢复布局' : '最大化 shell 窗口'}
                aria-label={paneMaximized ? '恢复布局' : '最大化 shell 窗口'}
                aria-pressed={paneMaximized}
                disabled={paneBusy}
                onClick={() => togglePaneMaximized(pane)}
              >
                {paneMaximized ? <Minimize2 /> : <Maximize2 />}
              </button>
            ) : null}
            <button title="关闭 pane" aria-label="关闭 pane" disabled={paneBusy} onClick={() => void closePane(pane)}>{paneBusy ? <LoaderCircle className="spinning" /> : <X />}</button>
          </header>}
            terminal={terminalVisible => <TerminalInputPane key={`${tab!.id}:${pane.id}`} visible={terminalVisible} tabId={tab!.id} pane={pane} active={activePaneId === pane.id} onFocus={() => setActivePaneId(pane.id)} onStatus={(status, code) => markStatus(pane.id, status, code)} />}
          />
        </section>
      );
    }
    const direction = node.axis === 'row' ? 'horizontal' : 'vertical';
    return (
      <div className={`terminal-split ${direction}`}>
        <div className="terminal-split-child" style={{ flexBasis: `${node.ratio * 100}%` }}>{renderLayout(node.first, [...path, 0])}</div>
        <div
          className={`terminal-split-divider ${direction}`}
          role="separator"
          aria-orientation={node.axis === 'row' ? 'vertical' : 'horizontal'}
          aria-valuemin={10}
          aria-valuemax={90}
          aria-valuenow={Math.round(node.ratio * 100)}
          onPointerDown={(event) => resizeSplit(event, node.axis, path)}
        />
        <div className="terminal-split-child" style={{ flexBasis: `${(1 - node.ratio) * 100}%` }}>{renderLayout(node.second, [...path, 1])}</div>
      </div>
    );
  };

  if (loading && !tab) {
    return <main className="terminal-workspace terminal-workspace-empty">{confirmationDialog}<LoaderCircle className="spinning" /><span>正在加载终端…</span></main>;
  }
  if (!tab) {
    return (
      <main className="terminal-workspace terminal-workspace-empty">
        {confirmationDialog}
        <div className="terminal-empty-guide">
          <h2>Terminal</h2>
          <p>从左侧新建一个终端，并通过终端标签页菜单修改窗口名称。</p>
        </div>
      </main>
    );
  }
  return (
    <main ref={terminalWorkspace} className="terminal-workspace">
      {confirmationDialog}
      {error ? <div className="terminal-workspace-error" role="alert">{error}<button onClick={() => setError('')}>关闭</button></div> : null}
      {maximizedPaneId ? (
        <nav className="terminal-pane-tabs-shell" aria-label="Terminal 窗口">
          <div className="terminal-pane-tabs" role="tablist">
            {naturalPaneOrder.map((paneId) => {
              const pane = tab.panes.find((item) => item.id === paneId);
              if (!pane) return null;
              const agentId = terminalPaneAgent(pane, detectedAgents);
              const title = terminalPaneTitle(pane, detectedAgents, terminalTitles);
              const selected = pane.id === maximizedPaneId;
              return (
                <button
                  key={pane.id}
                  type="button"
                  role="tab"
                  className={selected ? 'active' : ''}
                  aria-selected={selected}
                  title={`${title}\n${pane.cwd || '未知路径'}`}
                  onClick={() => selectMaximizedPane(pane.id)}
                >
                  <TerminalLifecycleDot className="terminal-pane-status" state={terminalPaneLifecycle(pane)} />
                  {agentId ? <AgentIcon agentId={agentId} /> : <SquareTerminal />}
                  <span>{title}</span>
                </button>
              );
            })}
          </div>
        </nav>
      ) : null}
      <div
        className={`terminal-layout ${maximizedPaneId ? 'pane-maximized' : ''}`}
        onDragOver={(event) => {
          if (!(event.target as Element).closest('.terminal-pane')) setPaneDropTarget(undefined);
        }}
        onDragLeave={(event) => {
          const next = event.relatedTarget;
          if (!(next instanceof Node) || !event.currentTarget.contains(next)) setPaneDropTarget(undefined);
        }}
      >
        {layout ? renderLayout(layout) : <div className="terminal-workspace-empty">这个终端没有可用的 pane。</div>}
      </div>
    </main>
  );
}
