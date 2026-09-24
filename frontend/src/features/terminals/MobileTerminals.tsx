import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { ArrowUp, Keyboard, KeyboardOff, List, Plus, ShieldCheck, TerminalSquare } from 'lucide-react';
import { TerminalPaneView } from './TerminalPaneView';
import { TerminalPanel } from './TerminalPanel';
import { TerminalLifecycleDot } from './TerminalLifecycleDot';
import { isCliTerminal } from './terminalPresentation';
import { AgentIcon } from '../agents/AgentIcon';
import { useTerminals } from './useTerminals';
import { useProjectTerminals } from './useProjectTerminals';
import { terminalApi } from './terminalApi';
import type { TerminalSort } from './TerminalScopeMenu';
import { useAowTabLocation, useAowTabLocationError } from '../../aow/AowTabEntry';
import { flattenTerminalTabs, terminalAccessoryKeys, terminalPaneAgent, type TerminalFrame, type TerminalPaneControls } from './terminalPresentation';
import { terminalPaneLifecycle, terminalPaneStatusMessage, type TerminalConnectionState } from './terminalState';
import type { TerminalPaneStatus, TerminalTab } from './types';
import type { AowProject, AowWorktree } from '../../aow/types';
import { MobileRefresh, MobileState } from '../../mobile/MobilePrimitives';
import { mobileRouteUrl, saveMobileValue, storedMobileValue } from '../../mobile/mobileState';
import { useMobileTerminalGestures } from './useMobileTerminalGestures';
import { useMobileTerminalSurface } from './useMobileTerminalSurface';
import { MobileTerminalCatalog } from './MobileTerminalCatalog';

type FlatTerminal = ReturnType<typeof flattenTerminalTabs>[number];
const DEFAULT_TERMINAL_FONT_SIZE = 12;
const terminalFontSize = (value: number) => Math.round(Math.min(28, Math.max(8, Number.isFinite(value) && value > 0 ? value : DEFAULT_TERMINAL_FONT_SIZE)) * 2) / 2;

function terminalPreferences(workspace: string) {
  const defaults = { opened: [] as string[], scopes: { user: false, cli: false }, sorts: { user: 'branch' as TerminalSort, cli: 'branch' as TerminalSort } };
  try {
    const saved = JSON.parse(storedMobileValue(`terminal-catalog.${workspace}`, '{}'));
    return {
      opened: Array.isArray(saved?.opened) ? saved.opened.filter((id: unknown): id is string => typeof id === 'string') : [],
      scopes: { user: saved?.scopes?.user === true, cli: saved?.scopes?.cli === true },
      sorts: { user: saved?.sorts?.user === 'createdAt' ? 'createdAt' as const : 'branch' as const,
        cli: saved?.sorts?.cli === 'createdAt' ? 'createdAt' as const : 'branch' as const },
    };
  } catch { return defaults; }
}

export function MobileTerminals({ project, worktree, visible, headerActions, initialTabId }: {
  initialTabId?: string; project: AowProject; worktree: AowWorktree; visible: boolean; headerActions: HTMLElement | null;
}) {
  const workspace = worktree.path;
  const terminals = useTerminals(workspace, visible);
  const [preferences, setPreferences] = useState(() => terminalPreferences(workspace));
  const [openedIds, setOpenedIds] = useState<Set<string>>(() => new Set([...preferences.opened, initialTabId, storedMobileValue(`terminal.${workspace}`).split(':')[0]].filter((id): id is string => Boolean(id))));
  const [catalogOpen, setCatalogOpen] = useState(false);
  const catalogId = useId();
  const catalogButton = useRef<HTMLButtonElement>(null);
  const closeCatalog = useCallback(() => setCatalogOpen(false), []);
  const [operationError, setOperationError] = useState('');
  const canShowAll = worktree.is_main && !project.builtin;
  const showAll = canShowAll && (preferences.scopes.user || preferences.scopes.cli);
  const hasHostedTabs = canShowAll && [...openedIds].some(id => !terminals.tabs.some(tab => tab.id === id));
  const projectTerminals = useProjectTerminals(project.worktrees, visible && (catalogOpen && showAll || hasHostedTabs));
  const detectedAgents = useMemo(() => ({ ...projectTerminals.agents, ...terminals.detectedAgents }), [projectTerminals.agents, terminals.detectedAgents]);
  const titles = useMemo(() => ({ ...projectTerminals.titles, ...terminals.terminalTitles }), [projectTerminals.titles, terminals.terminalTitles]);
  const entries = useMemo(() => flattenTerminalTabs([
    ...terminals.tabs.filter(tab => !isCliTerminal(tab) || openedIds.has(tab.id) || tab.id === initialTabId),
    ...projectTerminals.tabs.filter(tab => canShowAll && tab.workspace_root !== workspace && openedIds.has(tab.id)),
  ], detectedAgents, titles), [terminals.tabs, projectTerminals.tabs, openedIds, initialTabId, canShowAll, workspace, detectedAgents, titles]);
  const panelTabs = showAll ? [...terminals.tabs, ...projectTerminals.tabs.filter(tab => tab.workspace_root !== workspace
    && preferences.scopes[isCliTerminal(tab) ? 'cli' : 'user'])] : terminals.tabs;
  const [selected, setSelected] = useState(() => storedMobileValue(`terminal.${workspace}`));
  const [visited, setVisited] = useState<Set<string>>(new Set());
  const initialTabApplied = useRef<string | undefined>(undefined);
  const syncTabLocation = useAowTabLocation();
  const reportTabLocationError = useAowTabLocationError();
  const restoringHostedTab = hasHostedTabs && !projectTerminals.loaded && !projectTerminals.error
    && openedIds.has(selected.split(':')[0]) && !terminals.tabs.some(tab => tab.id === selected.split(':')[0]);
  const active = restoringHostedTab ? undefined : (initialTabId && initialTabApplied.current !== initialTabId
    ? entries.find(entry => entry.tab.id === initialTabId)
    : entries.find(entry => entry.key === selected)) ?? entries[0];
  const activeKey = active?.key;
  useEffect(() => {
    saveMobileValue(`terminal-catalog.${workspace}`, JSON.stringify({ ...preferences, opened: [...openedIds] }));
  }, [workspace, preferences, openedIds]);
  useEffect(() => {
    if (!terminals.loaded || terminals.loading || !projectTerminals.loaded || projectTerminals.error) return;
    const available = new Set([...terminals.tabs, ...projectTerminals.tabs].map(tab => tab.id));
    setOpenedIds(ids => [...ids].every(id => available.has(id)) ? ids : new Set([...ids].filter(id => available.has(id))));
  }, [terminals.loaded, terminals.loading, terminals.tabs, projectTerminals.loaded, projectTerminals.tabs, projectTerminals.error]);
  useEffect(() => { if (!visible) closeCatalog(); }, [visible, closeCatalog]);
  useEffect(() => {
    if (!initialTabId) { initialTabApplied.current = undefined; return; }
    if (initialTabApplied.current === initialTabId || !terminals.loaded || terminals.loading) return;
    initialTabApplied.current = initialTabId;
    const target = entries.find(entry => entry.tab.id === initialTabId);
    if (target) {
      setOpenedIds(ids => ids.has(target.tab.id) ? ids : new Set([...ids, target.tab.id]));
      setSelected(target.key);
    }
    else reportTabLocationError('该 Tab 已关闭或不存在。');
  }, [initialTabId, terminals.loaded, terminals.loading, entries, reportTabLocationError]);
  useEffect(() => {
    if (visible && terminals.loaded && !terminals.loading && !restoringHostedTab) {
      syncTabLocation(active?.tab.id, active ? '' : mobileRouteUrl({ workspace, view: 'terminal' }));
    }
  }, [initialTabId, visible, terminals.loaded, terminals.loading, restoringHostedTab, active?.tab.id, workspace, syncTabLocation]);
  useEffect(() => {
    if (activeKey) {
      setVisited((old) => old.has(activeKey) ? old : new Set([...old, activeKey]));
      saveMobileValue(`terminal.${workspace}`, activeKey);
    }
  }, [activeKey, workspace]);
  useEffect(() => {
    if (!visible) return;
    const refresh = () => { if (document.visibilityState === 'visible') void terminals.reload(); };
    const timer = window.setInterval(refresh, 15000);
    document.addEventListener('visibilitychange', refresh);
    return () => { clearInterval(timer); document.removeEventListener('visibilitychange', refresh); };
  }, [visible, terminals.reload]);
  useEffect(() => {
    if (visible) document.getElementById(`mobile-terminal-${activeKey}`)?.scrollIntoView({ inline: 'nearest', block: 'nearest' });
  }, [activeKey, visible]);
  const create = async () => {
    const tab = await terminals.create();
    if (tab?.panes[0]) setSelected(`${tab.id}:${tab.panes[0].id}`);
  };
  const reload = () => { void terminals.reload(); if (showAll || hasHostedTabs) projectTerminals.reload(); };
  const open = (tab: TerminalTab) => {
    const target = entries.find(entry => entry.tab.id === tab.id && entry.key === selected)
      ?? entries.find(entry => entry.tab.id === tab.id) ?? flattenTerminalTabs([tab], detectedAgents, titles)[0];
    if (!target) { setOperationError('该终端没有可打开的窗格，请刷新列表。'); return; }
    setOperationError('');
    setOpenedIds(ids => ids.has(tab.id) ? ids : new Set([...ids, tab.id]));
    setSelected(target.key);
    closeCatalog();
  };
  const terminate = async (id: string) => {
    setOperationError('');
    if (terminals.tabs.some(tab => tab.id === id)) await terminals.close(id);
    else {
      try { await terminalApi.close(id); projectTerminals.remove(id); }
      catch (reason) { setOperationError(reason instanceof Error ? reason.message : String(reason)); }
    }
  };
  const error = operationError || terminals.error || (hasHostedTabs ? projectTerminals.error : '');
  return <section className="mobile-terminal-page">
    {visible && headerActions && createPortal(<>
      <button ref={catalogButton} className="mobile-icon-button" aria-label="终端列表" aria-haspopup="dialog" aria-expanded={catalogOpen} aria-controls={catalogId}
        onClick={() => setCatalogOpen(true)}><List size={21} /></button>
      <MobileRefresh reload={reload} loading={terminals.loading || projectTerminals.loading} />
      <button className="mobile-icon-button" aria-label="新建终端" disabled={terminals.busy} onClick={() => void create()}><Plus size={21} /></button>
    </>, headerActions)}
    {error && <div className="mobile-inline-error" role="alert">{error}</div>}
    {visible && catalogOpen && <MobileTerminalCatalog id={catalogId} onClose={closeCatalog} returnFocus={catalogButton.current}>{container => <>
      {(operationError || terminals.error) && <div className="mobile-inline-error" role="alert">{operationError || terminals.error}</div>}
      <TerminalPanel menuContainer={container} tabs={panelTabs} activeId={active?.tab.id} detectedAgents={detectedAgents} titles={titles}
        openedIds={new Set(entries.map(entry => entry.tab.id))} onReload={reload} onTerminate={terminate} onOpen={open}
        onRebuild={async id => {
          const tab = await terminalApi.rebuild(id);
          terminals.replace(tab);
          projectTerminals.reload();
          if (active?.tab.id === id && tab.panes[0]) setSelected(`${id}:${tab.panes[0].id}`);
        }}
        worktrees={canShowAll ? project.worktrees : undefined} showAll={canShowAll ? preferences.scopes : undefined}
        onShowAllChange={(group, value) => setPreferences(current => ({ ...current, scopes: { ...current.scopes, [group]: value } }))}
        sortBy={preferences.sorts} onSortChange={(group, value) => setPreferences(current => ({ ...current, sorts: { ...current.sorts, [group]: value } }))}
        loading={showAll && projectTerminals.loading} error={showAll ? projectTerminals.error : undefined} />
    </>}</MobileTerminalCatalog>}
    {entries.length > 0 && <div className="mobile-terminal-tabs" role="tablist" aria-label="终端标签">
      {entries.map((entry) => <button key={entry.key} id={`mobile-terminal-${entry.key}`} role="tab" aria-selected={activeKey === entry.key}
        aria-controls={`mobile-terminal-panel-${entry.key}`} className={activeKey === entry.key ? 'active' : ''} onClick={() => setSelected(entry.key)}>
        {terminalPaneAgent(entry.pane, detectedAgents) ? <AgentIcon agentId={terminalPaneAgent(entry.pane, detectedAgents)} /> : <TerminalSquare size={15} />}
        <span>{entry.title}</span><TerminalLifecycleDot className="mobile-dot" state={terminalPaneLifecycle(entry.pane)} />
      </button>)}
    </div>}
    {(!entries.length || restoringHostedTab) && <MobileState loading={terminals.loading || restoringHostedTab} empty="从右上角列表打开已有终端，或点击 + 新建。" />}
    <div className="mobile-terminal-panels">
      {entries.filter((entry) => visited.has(entry.key) || entry.key === activeKey).map((entry) => <div className="mobile-terminal-panel"
        key={entry.key} id={`mobile-terminal-panel-${entry.key}`} role="tabpanel" aria-labelledby={`mobile-terminal-${entry.key}`} hidden={entry.key !== activeKey}>
        <MobileTerminal entry={entry} visible={visible && entry.key === activeKey}
          onStatus={(status, code) => (entry.tab.workspace_root === workspace ? terminals : projectTerminals).updatePaneStatus(entry.tab.id, entry.pane.id, status, code)} />
      </div>)}
    </div>
  </section>;
}

function MobileTerminal({ entry, visible, onStatus }: {
  entry: FlatTerminal; visible: boolean; onStatus: (status: TerminalPaneStatus, code?: number | null) => void;
}) {
  const controls = useRef<TerminalPaneControls | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLTextAreaElement>(null);
  const focusedInput = useRef<HTMLTextAreaElement | null>(null);
  const composing = useRef(false);
  const [requested, setRequested] = useState(Boolean(entry.pane.agent_terminal));
  const [force, setForce] = useState(false);
  const [connection, setConnection] = useState<TerminalConnectionState>('disconnected');
  const [message, setMessage] = useState<string>();
  const [keyboard, setKeyboard] = useState(false);
  const [draft, setDraft] = useState(() => storedMobileValue(`draft.${entry.key}`));
  const [fontSize, setFontSize] = useState(() => {
    const saved = storedMobileValue(`font-size.${entry.key}`);
    // Older versions saved the default even when the user never changed it.
    const legacyDefault = saved === '13' && storedMobileValue(`font-size-custom.${entry.key}`) !== '1';
    return terminalFontSize(Number(!saved || legacyDefault ? DEFAULT_TERMINAL_FONT_SIZE : saved));
  });
  const [frame, setFrame] = useState<TerminalFrame>({ cols: 80, rows: 24, width: 0, height: 360, top: 0, cursorRow: 0 });
  const surface = useMobileTerminalSurface(viewport, frame, visible);
  const ready = requested && visible && connection === 'connected';
  const hideKeyboard = () => {
    focusedInput.current?.blur();
    focusedInput.current = null;
    setKeyboard(false);
  };
  useEffect(() => { saveMobileValue(`draft.${entry.key}`, draft); }, [entry.key, draft]);
  useEffect(() => { saveMobileValue(`font-size.${entry.key}`, String(fontSize)); }, [entry.key, fontSize]);
  useEffect(() => { if (!visible || !ready) hideKeyboard(); }, [visible, ready]);
  const claim = () => {
    if (entry.pane.agent_terminal && entry.pane.agent_terminal.phase !== 'ready') return;
    if (!requested) { setForce(false); setRequested(true); }
    else controls.current?.takeControl();
  };
  const send = (key?: string) => {
    if (!ready || composing.current) return;
    if (draft && key !== '\u0003' && key !== '\u001b') {
      if (!controls.current?.send(draft, true)) return;
      setDraft('');
    }
    if (key) controls.current?.send(key);
  };
  const gestures = useMobileTerminalGestures(viewport, controls, frame, visible, fontSize, (value) => {
    saveMobileValue(`font-size-custom.${entry.key}`, '1');
    setFontSize(terminalFontSize(value));
  });
  const reportFrame = (next: TerminalFrame) => setFrame((old) => Object.keys(next).every((key) => old[key as keyof TerminalFrame] === next[key as keyof TerminalFrame]) ? old : next);
  const running = entry.pane.status === 'running';
  return <div className="mobile-terminal" onFocusCapture={(event) => {
    // xterm's hidden textarea and the composer share the keyboard button.
    if (event.target instanceof HTMLTextAreaElement) { focusedInput.current = event.target; setKeyboard(true); }
  }} onBlurCapture={(event) => {
    const next = event.relatedTarget;
    focusedInput.current = next instanceof HTMLTextAreaElement && event.currentTarget.contains(next) ? next : null;
    setKeyboard(focusedInput.current !== null);
  }}>
    <div className="mobile-terminal-display">
      <div className="mobile-terminal-viewport" ref={viewport} aria-label="终端画面，上下滑动滚动内容，双指缩放字体" {...gestures}>
        <div className="mobile-terminal-stage" ref={surface}>
          <TerminalPaneView pane={entry.pane} tabId={entry.tab.id} active={visible} visible={visible}
            sizing="container" renderer="dom" fontSize={fontSize} attachEnabled={visible && requested} forceOnAttach={force} autoFocus={false} mobileInput controlsRef={controls}
            onFocus={() => {}} onStatus={onStatus} onFrameChange={reportFrame}
            onConnectionChange={(state, detail) => { setConnection(state); setMessage(detail); if (state === 'connected') setForce(false); }} />
        </div>
      </div>
      {(!requested || running && !ready && connection !== 'observing') && <div className="mobile-terminal-welcome">
        <TerminalSquare size={28} /><strong>{entry.title}</strong>
        <p role="status">{requested ? terminalPaneStatusMessage(entry.pane, connection, message) : running ? '接管后同步终端画面与输入' : '进程已结束，可查看保留的输出'}</p>
        <button className="mobile-button mobile-terminal-claim" disabled={requested && connection === 'connecting'} onClick={claim}><ShieldCheck size={14} />{running ? requested ? '重新接管' : '接管终端' : '查看输出'}</button>
      </div>}
      {requested && running && connection === 'observing' && <button className="mobile-button mobile-terminal-observing-claim" disabled={Boolean(entry.pane.agent_terminal && entry.pane.agent_terminal.phase !== 'ready')} onClick={claim}><ShieldCheck size={14} />接管终端</button>}
    </div>
    <div className="mobile-terminal-input">
      <div className="mobile-accessory-keys" aria-label="终端辅助按键">
        {terminalAccessoryKeys.map((key) => <button key={key.label} disabled={!ready} aria-label={`发送 ${key.label}`}
          onPointerDown={(event) => event.preventDefault()} onClick={() => send(key.data)}>{key.label}</button>)}
      </div>
      <form className="mobile-composer" onSubmit={(event) => { event.preventDefault(); send('\r'); }}>
        <button type="button" aria-label={keyboard ? '收起键盘' : '打开键盘'} disabled={!ready} onPointerDown={(event) => event.preventDefault()} onMouseDown={(event) => event.preventDefault()} onClick={() => {
          if (keyboard) hideKeyboard(); else input.current?.focus({ preventScroll: true });
        }}>{keyboard ? <KeyboardOff size={21} /> : <Keyboard size={21} />}</button>
        <textarea ref={input} rows={1} aria-label="终端命令" placeholder={ready ? '输入命令，按 Enter 执行' : '接管后可输入'} value={draft} disabled={!ready}
          autoCorrect="off" autoCapitalize="off" spellCheck={false} enterKeyHint="send"
          onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }}
          onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing && !composing.current) { event.preventDefault(); send('\r'); }
          }} />
        <button className="mobile-send" type="submit" disabled={!ready} aria-label="执行命令" onPointerDown={(event) => event.preventDefault()}><ArrowUp size={20} /></button>
      </form>
    </div>
  </div>;
}
