import { t } from '../../lib/i18n';
import { useCallback, useEffect, useRef, useState, type DragEvent as ReactDragEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react';
import { Columns2, GripVertical, LoaderCircle, Maximize2, Minimize2, Rows2, SquareTerminal, X } from 'lucide-react';
import { terminalApi } from './terminalApi';
import type { TerminalAgentProcess, TerminalLayout, TerminalPane, TerminalPaneActivity, TerminalPaneStatus, TerminalSplitAxis, TerminalTab } from './types';
import { TerminalInputPane } from './TerminalInputPane';
import { TerminalAgentPane } from './TerminalAgentPane';
import { TerminalHostingButton } from './TerminalHostingButton';
import { AgentIcon } from '../agents/AgentIcon';
import { terminalPaneAgent, terminalPaneTitle } from './terminalPresentation';
import { terminalPaneLifecycle } from './terminalState';
import { TerminalLifecycleDot } from './TerminalLifecycleDot';
import { useTerminalPaneSelection } from './terminalViewState';
import { useConfirmation } from '../../components/ConfirmationDialog';

interface Props {
  visible: boolean;
  autoFocus?: boolean;
  focusRequest?: number;
  tab?: TerminalTab;
  loading: boolean;
  detectedAgents: Record<string, string | null>;
  terminalTitles: Record<string, string>;
  agentProcesses?: Record<string, TerminalAgentProcess>;
  terminalActivity?: Record<string, TerminalPaneActivity>;
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
  left: t("terminals.move_left"),
  right: t("terminals.move_right"),
  top: t("terminals.move_up"),
  bottom: t("terminals.move_down"),
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

export function TerminalWorkspace({ visible, autoFocus = true, focusRequest, tab, loading, detectedAgents, terminalTitles, agentProcesses = {}, terminalActivity = {}, onTabChange, onTabClosed, onPaneStatus, onReload }: Props) {
  const { confirm, confirmationDialog } = useConfirmation();
  const [layout, setLayout] = useState<TerminalLayout | null>(() => tab ? fallbackLayout(tab) : null);
  const [activePaneId, setActivePaneId] = useState<string>();
  const [paneFocusRequests, setPaneFocusRequests] = useState<Record<string, number>>({});
  // Explicit pane actions request focus even in embedded workspaces that suppress passive autofocus.
  const focusPane = useCallback((id: string) => {
    setActivePaneId(id);
    setPaneFocusRequests(current => ({ ...current, [id]: (current[id] ?? 0) + 1 }));
  }, []);
  const [draggingPaneId, setDraggingPaneId] = useState<string>();
  const [paneDropTarget, setPaneDropTarget] = useState<PaneDropTarget>();
  const [maximizedPaneIds, setMaximizedPaneIds] = useState<Record<string, string>>({});
  const [error, setError] = useState('');
  const [operation, setOperation] = useState('');
  const rebuildingRef = useRef(false);
  const layoutRef = useRef(layout);
  const layoutTabIdRef = useRef(tab?.id);
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
  const selection = useTerminalPaneSelection();
  const appliedSelection = useRef<typeof selection>(undefined);

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
    const sameTab = layoutTabIdRef.current === tab?.id;
    layoutTabIdRef.current = tab?.id;
    revisionRef.current = tab?.revision;
    const ids = paneIds(next);
    const draftIds = paneIds(layoutRef.current);
    // Snapshots acknowledge persisted layouts. Keep a newer local draft until
    // its save finishes, but still accept remote pane additions and removals.
    if (sameTab && (savingRef.current || pendingLayoutRef.current)) {
      if (ids.length === draftIds.length && ids.every(id => draftIds.includes(id))) return;
      // A remote structural change invalidates queued saves of the old pane set.
      saveEpochRef.current += 1;
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = undefined;
      pendingLayoutRef.current = undefined;
      flushRequestedRef.current = false;
      savingRef.current = false;
    }
    setLayout(next);
    layoutRef.current = next;
    setActivePaneId((current) => current && ids.includes(current)
      ? current
      : maximizedPaneId && ids.includes(maximizedPaneId) ? maximizedPaneId : ids[0]);
    setDraggingPaneId(undefined);
    draggingPaneIdRef.current = undefined;
    setPaneDropTarget(undefined);
    setError('');
  }, [tab?.id, tab?.layout, tab?.revision]);

  useEffect(() => {
    if (!visible || !selection || appliedSelection.current === selection || tab?.id !== selection.tabId
      || !tab.panes.some(pane => pane.id === selection.paneId)) return;
    appliedSelection.current = selection;
    focusPane(selection.paneId);
    if (maximizedPaneId) setMaximizedPaneId(tab.id, selection.paneId);
  }, [visible, selection, tab, maximizedPaneId, focusPane]);

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
        if (!stillActive && tabRef.current?.id === currentTab.id) return;
        if (stillActive) revisionRef.current = updated.revision;
        // Shared state contains only committed server data; unsaved divider
        // movements remain in this workspace until their own response arrives.
        onTabChange(updated);
      }
    } catch (reason) {
      if (saveEpochRef.current === saveEpoch) {
        setError(t("terminals.failed_to_save_layout", { value0: message(reason) }));
        if (!pendingLayoutRef.current) {
          const current = tabRef.current;
          const next = current ? fallbackLayout(current) : null;
          layoutRef.current = next;
          setLayout(next);
        }
      }
    } finally {
      if (saveEpochRef.current === saveEpoch) {
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
      focusPane(sourcePaneId);
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
        focusPane(ids.find((id) => !tab.panes.some((oldPane) => oldPane.id === id)) ?? pane.id);
      } else {
        onReload();
      }
    } catch (reason) {
      setError(t("terminals.failed_to_split_pane", { value0: message(reason) }));
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
    if (!tab || tab.panes.length <= 1 || operation) return;
    focusPane(pane.id);
    setDraggingPaneId(undefined);
    draggingPaneIdRef.current = undefined;
    setPaneDropTarget(undefined);
    setMaximizedPaneId(tab.id, (current) => current === pane.id ? undefined : pane.id);
  };

  const selectMaximizedPane = (paneId: string) => {
    if (!tab || !maximizedPaneId || paneId === maximizedPaneId) return;
    focusPane(paneId);
    setMaximizedPaneId(tab.id, paneId);
  };

  const closePane = async (pane: TerminalPane) => {
    if (!tab) return;
    const closingLastPane = tab.panes.length === 1;
    if (!await confirm({
      title: closingLastPane ? t("terminals.close_terminal") : t("terminals.close_terminal_window"),
      description: closingLastPane
        ? t("terminals.this_is_the_last_window_of_closing_it_also", { value0: tab.name })
        : t("terminals.the_following_terminal_window_will_close"),
      items: [{ id: pane.id, label: terminalPaneTitle(pane, detectedAgents, terminalTitles, terminalActivity, agentProcesses), detail: terminalActivity[pane.id]?.cwd || pane.cwd, icon: <SquareTerminal /> }],
      warning: t("terminals.running_commands_in_this_window_will_be_terminated_this"),
      confirmLabel: closingLastPane ? t("terminals.close_terminal_2") : t("terminals.close_window"),
      danger: true,
    })) return;
    if (tabRef.current?.id !== tab.id || !tabRef.current.panes.some((item) => item.id === pane.id)) return;
    setOperation(`close:${pane.id}`);
    setError('');
    try {
      const updated = await terminalApi.closePane(tab.id, pane.id);
      if (updated) {
        if (maximizedPaneId === pane.id || activePaneId === pane.id) {
          const currentIds = paneIds(layoutRef.current);
          const closedIndex = currentIds.indexOf(pane.id);
          const remainingIds = paneIds(fallbackLayout(updated));
          const nextPaneId = remainingIds[Math.min(Math.max(closedIndex, 0), remainingIds.length - 1)];
          if (maximizedPaneId) setMaximizedPaneId(tab.id, updated.panes.length > 1 ? nextPaneId : undefined);
          if (nextPaneId) focusPane(nextPaneId);
        }
        onTabChange(updated);
      } else {
        setMaximizedPaneId(tab.id, undefined);
        onTabClosed(tab.id);
      }
    } catch (reason) {
      setError(t("terminals.failed_to_close_pane", { value0: message(reason) }));
    } finally {
      setOperation('');
    }
  };

  const canRebuild = Boolean(tab?.panes.length && tab.panes.every(pane => pane.status !== 'running'));
  const rebuild = async () => {
    if (!tab || !canRebuild || operation || rebuildingRef.current) return;
    rebuildingRef.current = true;
    setOperation('rebuild');
    setError('');
    try {
      const updated = await terminalApi.rebuild(tab.id);
      onTabChange(updated);
      if (updated.panes[0]) focusPane(updated.panes[0].id);
    } catch (reason) {
      setError(message(reason));
    } finally {
      rebuildingRef.current = false;
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
      if (!pane) return <div className="terminal-pane-missing">{t("navigation.pane")}{node.pane_id} {t("terminals.not_found")}</div>;
      const agentId = terminalPaneAgent(pane, detectedAgents);
      const title = terminalPaneTitle(pane, detectedAgents, terminalTitles, terminalActivity, agentProcesses);
      const paneBusy = operation === 'rebuild' || operation.endsWith(`:${pane.id}`);
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
            onPointerDown={event => {
              if (event.button === 0 && !(event.target instanceof Element && event.target.closest('button'))) focusPane(pane.id);
            }}
            onDragStart={(event) => startPaneDrag(event, pane.id)}
            onDragEnd={finishPaneDrag}
            onDoubleClick={(event) => {
              if (event.target instanceof Element && event.target.closest('button')) return;
              togglePaneMaximized(pane);
            }}
          >
            <GripVertical className="terminal-pane-drag-handle" aria-label={t("terminals.drag_pane_to_rearrange_layout")} />
            <TerminalLifecycleDot className="terminal-pane-status" state={terminalPaneLifecycle(pane)} />
            {agentId ? <AgentIcon agentId={agentId} /> : <SquareTerminal />}
            <span className="terminal-pane-name" title={t("terminals.path", { value0: title, value1: terminalActivity[pane.id]?.cwd || pane.cwd || t("pr.unknown") })}>
              {title}
            </span>
            {sessionButton}
            {agentId && <TerminalHostingButton tab={tab!} pane={pane} onChange={onTabChange} />}
            <button title={t("terminals.split_right", { value0: splitShortcutLabel('row') })} aria-label={t("terminals.split_right_2")} disabled={paneBusy} onClick={() => void split(pane, 'row')}><Columns2 /></button>
            <button title={t("terminals.split_down", { value0: splitShortcutLabel('column') })} aria-label={t("terminals.split_down_2")} disabled={paneBusy} onClick={() => void split(pane, 'column')}><Rows2 /></button>
            {tab!.panes.length > 1 ? (
              <button
                title={paneMaximized ? t("terminals.restore_layout") : t("terminals.maximize_shell_window")}
                aria-label={paneMaximized ? t("terminals.restore_layout") : t("terminals.maximize_shell_window")}
                aria-pressed={paneMaximized}
                disabled={paneBusy}
                onClick={() => togglePaneMaximized(pane)}
              >
                {paneMaximized ? <Minimize2 /> : <Maximize2 />}
              </button>
            ) : null}
            <button title={t("terminals.close_pane")} aria-label={t("terminals.close_pane")} disabled={paneBusy} onClick={() => void closePane(pane)}>{paneBusy ? <LoaderCircle className="spinning" /> : <X />}</button>
          </header>}
            terminal={terminalVisible => <TerminalInputPane key={`${tab!.id}:${pane.id}`} visible={terminalVisible} autoFocus={autoFocus} focusRequest={(focusRequest ?? 0) + (paneFocusRequests[pane.id] ?? 0)} tabId={tab!.id} pane={pane} active={activePaneId === pane.id} onFocus={() => setActivePaneId(pane.id)} onStatus={(status, code) => markStatus(pane.id, status, code)}
              onRebuild={canRebuild ? () => void rebuild() : undefined} rebuilding={operation === 'rebuild'} />}
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
    return <main className="terminal-workspace terminal-workspace-empty">{confirmationDialog}<LoaderCircle className="spinning" /><span>{t("terminals.loading_terminal")}</span></main>;
  }
  if (!tab) {
    return (
      <main className="terminal-workspace terminal-workspace-empty">
        {confirmationDialog}
        <div className="terminal-empty-guide">
          <h2>{t("navigation.terminal")}</h2>
          <p>{t("terminals.create_a_terminal_from_the_left_sidebar_rename_windows")}</p>
        </div>
      </main>
    );
  }
  return (
    <main ref={terminalWorkspace} className="terminal-workspace">
      {confirmationDialog}
      {error ? <div className="terminal-workspace-error" role="alert">{error}<button onClick={() => setError('')}>{t("aow.close")}</button></div> : null}
      {maximizedPaneId ? (
        <nav className="terminal-pane-tabs-shell" aria-label={t("terminals.terminal_window")}>
          <div className="terminal-pane-tabs" role="tablist">
            {naturalPaneOrder.map((paneId) => {
              const pane = tab.panes.find((item) => item.id === paneId);
              if (!pane) return null;
              const agentId = terminalPaneAgent(pane, detectedAgents);
              const title = terminalPaneTitle(pane, detectedAgents, terminalTitles, terminalActivity, agentProcesses);
              const selected = pane.id === maximizedPaneId;
              return (
                <button
                  key={pane.id}
                  type="button"
                  role="tab"
                  className={selected ? 'active' : ''}
                  aria-selected={selected}
                  title={`${title}\n${terminalActivity[pane.id]?.cwd || pane.cwd || t("common.unknown_path")}`}
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
        {layout ? renderLayout(layout) : <div className="terminal-workspace-empty">{t("terminals.this_terminal_has_no_available_panes")}</div>}
      </div>
    </main>
  );
}
