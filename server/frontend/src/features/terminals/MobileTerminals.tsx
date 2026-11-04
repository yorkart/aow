import { t } from '../../lib/i18n';
import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { ArrowUp, Keyboard, KeyboardOff, List, ShieldCheck, TerminalSquare } from 'lucide-react';
import { TerminalPaneView } from './TerminalPaneView';
import { TerminalPanel } from './TerminalPanel';
import { TerminalHostingButton } from './TerminalHostingButton';
import { TerminalHostingDetails, hostingStatus } from './TerminalHostingStatus';
import { TerminalLifecycleDot } from './TerminalLifecycleDot';
import { isCliTerminal } from './terminalPresentation';
import { AgentIcon } from '../agents/AgentIcon';
import { useTerminals } from './useTerminals';
import { useProjectTerminals } from './useProjectTerminals';
import { terminalApi } from './terminalApi';
import type { InstanceSort } from '../instances/InstanceScopeMenu';
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
import { MobileTerminalNewMenu } from './MobileTerminalNewMenu';

type FlatTerminal = ReturnType<typeof flattenTerminalTabs>[number];
const DEFAULT_TERMINAL_FONT_SIZE = 12;
const terminalFontSize = (value: number) => Math.round(Math.min(28, Math.max(8, Number.isFinite(value) && value > 0 ? value : DEFAULT_TERMINAL_FONT_SIZE)) * 2) / 2;

function terminalPreferences(workspace: string) {
  const defaults = { opened: [] as string[], showAll: false, sortBy: 'branch' as InstanceSort };
  try {
    const saved = JSON.parse(storedMobileValue(`terminal-catalog.${workspace}`, '{}'));
    return {
      opened: Array.isArray(saved?.opened) ? saved.opened.filter((id: unknown): id is string => typeof id === 'string') : [],
      showAll: typeof saved?.showAll === 'boolean' ? saved.showAll : saved?.scopes?.user === true || saved?.scopes?.cli === true,
      sortBy: (saved?.sortBy ?? (saved?.sorts?.user === 'createdAt' || saved?.sorts?.cli === 'createdAt' ? 'createdAt' : 'branch')) === 'createdAt'
        ? 'createdAt' as const : 'branch' as const,
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
  const showAll = canShowAll && preferences.showAll;
  const hasHostedTabs = canShowAll && [...openedIds].some(id => !terminals.tabs.some(tab => tab.id === id));
  const projectTerminals = useProjectTerminals(project.worktrees, visible && (catalogOpen && canShowAll || hasHostedTabs));
  const detectedAgents = useMemo(() => ({ ...projectTerminals.agents, ...terminals.detectedAgents }), [projectTerminals.agents, terminals.detectedAgents]);
  const titles = useMemo(() => ({ ...projectTerminals.titles, ...terminals.terminalTitles }), [projectTerminals.titles, terminals.terminalTitles]);
  const activity = useMemo(() => ({ ...projectTerminals.activity, ...terminals.terminalActivity }), [projectTerminals.activity, terminals.terminalActivity]);
  const processes = useMemo(() => ({ ...projectTerminals.processes, ...terminals.agentProcesses }), [projectTerminals.processes, terminals.agentProcesses]);
  const entries = useMemo(() => flattenTerminalTabs([
    ...terminals.tabs.filter(tab => !isCliTerminal(tab) || openedIds.has(tab.id) || tab.id === initialTabId),
    ...projectTerminals.tabs.filter(tab => canShowAll && tab.workspace_root !== workspace && openedIds.has(tab.id)),
  ], detectedAgents, titles, activity, processes), [terminals.tabs, projectTerminals.tabs, openedIds, initialTabId, canShowAll, workspace, detectedAgents, titles, activity, processes]);
  const panelTabs = showAll ? [...terminals.tabs, ...projectTerminals.tabs.filter(tab => tab.workspace_root !== workspace)] : terminals.tabs;
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
    else reportTabLocationError(t("aow.this_tab_has_been_closed_or_no_longer_exists"));
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
    if (visible) document.getElementById(`mobile-terminal-${activeKey}`)?.scrollIntoView({ inline: 'nearest', block: 'nearest' });
  }, [activeKey, visible]);
  const create = async (agentId?: string) => {
    setOperationError('');
    const tab = await terminals.create(agentId);
    if (tab?.panes[0]) setSelected(`${tab.id}:${tab.panes[0].id}`);
  };
  const reload = () => { void terminals.reload(); };
  const open = (tab: TerminalTab, paneId?: string) => {
    const target = (paneId ? flattenTerminalTabs([tab], detectedAgents, titles, activity, processes).find(entry => entry.pane.id === paneId) : undefined)
      ?? entries.find(entry => entry.tab.id === tab.id && entry.key === selected)
      ?? entries.find(entry => entry.tab.id === tab.id) ?? flattenTerminalTabs([tab], detectedAgents, titles, activity, processes)[0];
    if (!target) { setOperationError(t("terminals.this_terminal_has_no_available_panes_refresh_the_list")); return; }
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
      {active && terminalPaneAgent(active.pane, detectedAgents) && <TerminalHostingButton tab={active.tab} pane={active.pane} onChange={tab => { if (tab.workspace_root === workspace) terminals.replace(tab); else projectTerminals.reload(); }} />}
      <button ref={catalogButton} className="mobile-icon-button" aria-label={t("terminals.terminal_list")} aria-haspopup="dialog" aria-expanded={catalogOpen} aria-controls={catalogId}
        onClick={() => setCatalogOpen(true)}><List size={21} /></button>
      <MobileRefresh reload={reload} loading={terminals.loading || projectTerminals.loading} />
      <MobileTerminalNewMenu busy={terminals.busy} onCreate={agentId => void create(agentId)} />
    </>, headerActions)}
    {error && <div className="mobile-inline-error" role="alert">{error}</div>}
    {visible && catalogOpen && <MobileTerminalCatalog id={catalogId} onClose={closeCatalog} returnFocus={catalogButton.current}>{container => <>
      {(operationError || terminals.error) && <div className="mobile-inline-error" role="alert">{operationError || terminals.error}</div>}
      <TerminalPanel menuContainer={container} tabs={panelTabs} descendantTabs={[...terminals.tabs, ...projectTerminals.tabs.filter(tab => tab.workspace_root !== workspace)]}
        activeId={active?.tab.id} detectedAgents={detectedAgents} titles={titles} onOpenPane={open}
        openedIds={new Set(entries.map(entry => entry.tab.id))} onReload={reload} onTerminate={terminate} onOpen={open}
        onRebuild={async id => {
          const tab = await terminalApi.rebuild(id);
          terminals.replace(tab);
          projectTerminals.reload();
          if (active?.tab.id === id && tab.panes[0]) setSelected(`${id}:${tab.panes[0].id}`);
        }}
        worktrees={canShowAll ? project.worktrees : undefined} showAll={showAll}
        onShowAllChange={showAll => setPreferences(current => ({ ...current, showAll }))}
        sortBy={preferences.sortBy} onSortChange={sortBy => setPreferences(current => ({ ...current, sortBy }))}
        loading={showAll && projectTerminals.loading}
        error={showAll && projectTerminals.error !== (operationError || terminals.error) ? projectTerminals.error : undefined} />
    </>}</MobileTerminalCatalog>}
    {entries.length > 0 && <div className="mobile-terminal-tabs" role="tablist" aria-label={t("terminals.terminal_tabs")}>
      {entries.map((entry) => <button key={entry.key} id={`mobile-terminal-${entry.key}`} role="tab" aria-selected={activeKey === entry.key}
        aria-controls={`mobile-terminal-panel-${entry.key}`} className={activeKey === entry.key ? 'active' : ''} onClick={() => setSelected(entry.key)}>
        {terminalPaneAgent(entry.pane, detectedAgents) ? <AgentIcon agentId={terminalPaneAgent(entry.pane, detectedAgents)} /> : <TerminalSquare size={15} />}
        <span>{entry.title}</span><TerminalLifecycleDot className="mobile-dot" state={terminalPaneLifecycle(entry.pane)} />
      </button>)}
    </div>}
    {(!entries.length || restoringHostedTab) && <MobileState loading={terminals.loading || restoringHostedTab} empty={t("terminals.open_existing_terminals_from_the_top_right_list_or")} />}
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
  const [requested, setRequested] = useState(Boolean(entry.pane.agent_terminal || entry.pane.hosting));
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
  const ready = requested && visible && connection === 'connected' && !entry.pane.hosting;
  useEffect(() => { if (entry.pane.hosting) setRequested(true); }, [entry.pane.hosting?.id]);
  useLayoutEffect(() => {
    const node = input.current;
    if (!node || !visible) return;
    let width = 0;
    const resize = () => {
      if (!node.clientWidth || node.clientWidth === width) return;
      width = node.clientWidth;
      const scrollTop = node.scrollTop;
      // Measure wrapped content from one row; CSS caps the height at five rows.
      node.style.height = 'auto';
      node.style.height = `${node.scrollHeight}px`;
      node.scrollTop = scrollTop;
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(node);
    return () => observer.disconnect();
  }, [draft, visible]);
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
      <div className="mobile-terminal-viewport" ref={viewport} aria-label={t("terminals.terminal_screen_swipe_vertically_to_scroll_pinch_to_change")} {...gestures}>
        <div className="mobile-terminal-stage" ref={surface}>
          <TerminalPaneView pane={entry.pane} tabId={entry.tab.id} active={visible} visible={visible}
            sizing="container" renderer="dom" fontSize={fontSize} attachEnabled={visible && requested} forceOnAttach={force} autoFocus={false} mobileInput controlsRef={controls}
            onFocus={() => {}} onStatus={onStatus} onFrameChange={reportFrame}
            onConnectionChange={(state, detail) => { setConnection(state); setMessage(detail); if (state === 'connected') setForce(false); }} />
        </div>
      </div>
      {!entry.pane.hosting && (!requested || running && !ready && connection !== 'observing') && <div className="mobile-terminal-welcome">
        <TerminalSquare size={28} /><strong>{entry.title}</strong>
        <p role="status">{requested ? terminalPaneStatusMessage(entry.pane, connection, message) : running ? t("terminals.take_control_to_sync_terminal_display_and_input") : t("terminals.process_ended_retained_output_is_available")}</p>
        <button className="mobile-button mobile-terminal-claim" disabled={requested && connection === 'connecting'} onClick={claim}><ShieldCheck size={14} />{running ? requested ? t("terminals.retake_control") : t("terminals.take_terminal_control") : t("terminals.view_output")}</button>
      </div>}
      {entry.pane.hosting && <div className="mobile-terminal-hosting"><div><span>{hostingStatus(entry.pane.hosting)}</span><button className="mobile-button" onClick={claim}>{t("acp-control.take_control")}</button></div><TerminalHostingDetails hosting={entry.pane.hosting} /></div>}
      {!entry.pane.hosting && requested && running && connection === 'observing' && <button className="mobile-button mobile-terminal-observing-claim" disabled={Boolean(entry.pane.agent_terminal && entry.pane.agent_terminal.phase !== 'ready')} onClick={claim}><ShieldCheck size={14} />{t("terminals.take_terminal_control")}</button>}
    </div>
    <div className="mobile-terminal-input">
      <div className="mobile-accessory-keys" aria-label={t("terminals.terminal_helper_keys")}>
        {terminalAccessoryKeys.map((key) => <button key={key.label} disabled={!ready} aria-label={t("terminals.send", { value0: key.label })}
          onPointerDown={(event) => event.preventDefault()} onClick={() => send(key.data)}>{key.label}</button>)}
      </div>
      <form className="mobile-composer" onSubmit={(event) => { event.preventDefault(); send('\r'); }}>
        <button type="button" aria-label={keyboard ? t("terminals.hide_keyboard") : t("terminals.show_keyboard")} disabled={!ready} onPointerDown={(event) => event.preventDefault()} onMouseDown={(event) => event.preventDefault()} onClick={() => {
          if (keyboard) hideKeyboard(); else input.current?.focus({ preventScroll: true });
        }}>{keyboard ? <KeyboardOff size={21} /> : <Keyboard size={21} />}</button>
        <textarea ref={input} rows={1} aria-label={t("terminals.terminal_command")} placeholder={ready ? t("terminals.enter_a_command_and_press_enter_to_run") : t("terminals.take_control_to_type")} value={draft} disabled={!ready}
          autoCorrect="off" autoCapitalize="off" spellCheck={false} enterKeyHint="send"
          onCompositionStart={() => { composing.current = true; }} onCompositionEnd={() => { composing.current = false; }}
          onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing && !composing.current) { event.preventDefault(); send('\r'); }
          }} />
        <button className="mobile-send" type="submit" disabled={!ready} aria-label={t("terminals.run_command")} onPointerDown={(event) => event.preventDefault()}><ArrowUp size={20} /></button>
      </form>
    </div>
  </div>;
}
