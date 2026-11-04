import { t, getLanguage } from '../../lib/i18n';
import { appSessionStorage } from '../../lib/basePath';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { LoaderCircle, MessageSquare, RefreshCw, SquareTerminal } from 'lucide-react';
import type { AgentSessionSnapshot, AowAgentSession } from '../sessions/types';
import type { TerminalAgentProcess, TerminalPaneSessions } from './types';
import { terminalApi } from './terminalApi';
import { sessionsApi } from '../sessions/api';
import { agentSessionTitleSource } from '../agents/agentTypes';
import { sessionSearch, terminalSessionCandidates, terminalSessionTitle } from './terminalSessionMatching';
import { SessionSnapshotView } from '../sessions/SessionSnapshotView';
import { SessionShareButton } from '../sessions/SessionShareButton';
import './terminal-agent-session.css';

interface Props {
  tabId: string;
  paneId: string;
  agentId?: string | null;
  title: string;
  cwd: string;
  process?: TerminalAgentProcess;
  visible: boolean;
  header: (sessionButton: ReactNode) => ReactNode;
  terminal: (visible: boolean) => ReactNode;
}

interface Association {
  key: string;
  data?: TerminalPaneSessions;
  selected?: AowAgentSession;
  choosing: boolean;
  manual: boolean;
}

function savedAssociation(storageKey: string, identity: string, liveId: string | null): string | undefined {
  try {
    const saved = JSON.parse(appSessionStorage.getItem(storageKey) ?? 'null');
    return saved?.key === identity && saved.liveId === liveId && typeof saved.id === 'string' ? saved.id : undefined;
  } catch { return undefined; }
}

function relativeTime(value: string) {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return value;
  const seconds = Math.round((timestamp - Date.now()) / 1000);
  if (seconds === 0) return t("terminals.just_now");
  const ranges: [Intl.RelativeTimeFormatUnit, number][] = [
    ['year', 31_536_000], ['month', 2_592_000], ['week', 604_800],
    ['day', 86_400], ['hour', 3_600], ['minute', 60],
  ];
  const formatter = new Intl.RelativeTimeFormat(getLanguage(), { numeric: 'always' });
  for (const [unit, size] of ranges) {
    if (Math.abs(seconds) >= size) return formatter.format(Math.round(seconds / size), unit);
  }
  return formatter.format(seconds, 'second');
}

export function TerminalAgentPane({ tabId, paneId, agentId, title, cwd, process, visible, header, terminal }: Props) {
  const titleSource = agentSessionTitleSource(agentId);
  const supported = !!titleSource;
  const storageKey = `terminal.agent-session.${tabId}.${paneId}`;
  // Native display titles can change without changing the session identity.
  const identity = JSON.stringify([tabId, paneId, agentId, process?.pid, process?.start_time, process?.cwd ?? cwd,
    titleSource === 'native' ? '' : terminalSessionTitle(title, process?.cwd ?? cwd)]);
  const [open, setOpen] = useState(false);
  const [association, setAssociation] = useState<Association>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [query, setQuery] = useState('');
  const [showAll, setShowAll] = useState(false);
  const [snapshotState, setSnapshotState] = useState<{ key: string; snapshot?: AgentSessionSnapshot; loading: boolean; error?: string }>();
  const [revision, setRevision] = useState(0);
  const currentIdentity = useRef(identity);
  currentIdentity.current = identity;
  const request = useRef<AbortController | undefined>(undefined);
  const current = association?.key === identity ? association : undefined;
  const selected = current?.choosing ? undefined : current?.selected;
  const showing = open && supported;

  const refresh = useCallback(async () => {
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    setLoading(true);
    setError('');
    try {
      const data = await terminalApi.paneSessions(tabId, paneId, controller.signal);
      if (controller.signal.aborted || currentIdentity.current !== identity) return;
      // Ignore results from a newer process until the shared metadata poll
      // catches up; a response must not bind an old pane generation.
      if (data.agent !== agentId || (process && (data.process?.pid !== process.pid
        || data.process.start_time !== process.start_time))) {
        setAssociation(undefined);
        setError(t("terminals.agent_process_changed_refresh_the_session_list"));
        return;
      }
      const candidates = terminalSessionCandidates(data);
      setAssociation(previous => {
        const same = previous?.key === identity ? previous : undefined;
        const liveChanged = same?.data?.live_session_id !== data.live_session_id;
        const savedId = savedAssociation(storageKey, identity, data.live_session_id);
        const retainedId = !liveChanged && same?.manual && same.selected ? same.selected.id : savedId;
        const retained = data.sessions.find(session => session.id === retainedId);
        return {
          key: identity, data,
          selected: retained ?? (same?.choosing ? undefined : candidates.automatic),
          choosing: same?.choosing ?? false,
          manual: !!retained && (same?.manual === true || retained.id === savedId),
        };
      });
      setRevision(value => value + 1);
    } catch (reason) {
      if (!controller.signal.aborted && currentIdentity.current === identity) {
        setError(reason instanceof Error ? reason.message : String(reason));
      }
    } finally {
      if (!controller.signal.aborted && currentIdentity.current === identity) setLoading(false);
    }
  }, [tabId, paneId, identity, agentId, process?.pid, process?.start_time]);

  useEffect(() => {
    setQuery('');
    setShowAll(false);
    if (!supported) { setOpen(false); setAssociation(undefined); }
  }, [identity, supported]);

  useEffect(() => {
    if (!showing || !visible) return;
    const update = () => { if (document.visibilityState === 'visible') void refresh(); };
    update();
    const timer = window.setInterval(update, 10_000);
    document.addEventListener('visibilitychange', update);
    return () => {
      request.current?.abort();
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', update);
    };
  }, [showing, visible, refresh]);

  const snapshotKey = selected ? `${identity}:${selected.id}` : '';
  useEffect(() => {
    if (!selected || !showing || !visible || error) return;
    const controller = new AbortController();
    setSnapshotState(previous => ({ key: snapshotKey, snapshot: previous?.key === snapshotKey ? previous.snapshot : undefined, loading: true }));
    void sessionsApi.agentSessionSnapshot(selected, selected.cwd, controller.signal).then(snapshot => {
      if (!controller.signal.aborted) setSnapshotState({ key: snapshotKey, snapshot, loading: false });
    }).catch(reason => {
      if (!controller.signal.aborted) setSnapshotState(previous => ({
        key: snapshotKey, snapshot: previous?.key === snapshotKey ? previous.snapshot : undefined,
        loading: false, error: reason instanceof Error ? reason.message : String(reason),
      }));
    });
    return () => controller.abort();
  }, [snapshotKey, revision, showing, visible, error]);

  const snapshot = snapshotState?.key === snapshotKey ? snapshotState : undefined;
  const candidates = current?.data ? terminalSessionCandidates(current.data) : undefined;
  const matches = candidates?.matches ?? [];
  const sessions = current?.data?.sessions ?? [];
  const displayed = sessionSearch(showAll || !matches.length ? sessions : matches, query);

  const choose = (session: AowAgentSession) => {
    try { appSessionStorage.setItem(storageKey, JSON.stringify({ key: identity, id: session.id, liveId: current?.data?.live_session_id ?? null })); } catch { /* Storage may be unavailable. */ }
    setAssociation(previous => previous?.key === identity
      ? { ...previous, selected: session, choosing: false, manual: true } : previous);
  };
  const reselect = () => {
    setAssociation(previous => previous?.key === identity ? { ...previous, choosing: true } : previous);
    setQuery('');
    setShowAll(true);
    void refresh();
  };

  return <>
    {header(supported ? <button className="terminal-agent-session-button" type="button"
      title={showing ? t("terminals.back_to_terminal") : t("terminals.switch_to_session_details")} aria-label={showing ? t("terminals.back_to_terminal") : t("terminals.switch_to_session_details")}
      aria-pressed={showing} onClick={() => setOpen(value => !value)}>
      {showing ? <SquareTerminal /> : <MessageSquare />}<span>{showing ? t("terminals.terminal") : t("navigation.conversation")}</span>
    </button> : null)}
    <div className="terminal-pane-surface" hidden={showing}>
      {terminal(visible && !showing)}
    </div>
    {showing ? <div className="terminal-agent-session-surface">
      <div className="terminal-agent-session-toolbar">
        <span title={selected?.session_id ?? current?.data?.cwd ?? cwd}>
          {selected ? `${current?.manual ? t("aow.selected") : current?.data?.live_session_id ? t("common.current_session") : t("common.title_match")} · ${selected.session_id}` : t("terminals.select_current_session")}
        </span>
        {selected ? <button type="button" onClick={reselect}>{t("terminals.choose_again")}</button> : null}
        <button type="button" title={t("terminals.refresh_session_list")} aria-label={t("terminals.refresh_session_list")} disabled={loading} onClick={() => void refresh()}><RefreshCw className={loading ? 'spinning' : ''} /></button>
      </div>
      {error ? <div className="terminal-agent-session-error" role="alert">{error}</div> : null}
      {selected ? <SessionSnapshotView session={selected} snapshot={snapshot?.snapshot} loading={snapshot?.loading ?? true}
        actions={snapshot?.snapshot && <SessionShareButton session={selected} workspacePath={selected.cwd} />}
        error={snapshot?.error} onRefresh={() => void refresh()} /> : <div className="terminal-agent-session-picker">
        <p>{loading && !current?.data ? t("terminals.finding_current_session")
          : current?.data?.live_session_id && !matches.length ? t("terminals.current_session_identified_but_records_are_not_ready_refresh")
          : matches.length ? t("terminals.found_sessions_with_matching_titles_confirm_which_one_is", { value0: matches.length })
          : t("terminals.no_unique_match_choose_a_session_from_the_current")}</p>
        <div className="terminal-agent-session-filter">
          <input aria-label={t("terminals.search_session_title_or_id")} placeholder={t("terminals.search_session_title_or_id")} value={query} onChange={event => setQuery(event.target.value)} />
          {matches.length > 0 ? <button type="button" onClick={() => setShowAll(value => !value)}>{showAll ? t("terminals.only_matching_titles") : t("terminals.show_all_sessions")}</button> : null}
        </div>
        <div className="terminal-agent-session-directory" title={current?.data?.cwd ?? cwd}>{current?.data?.cwd ?? cwd}</div>
        <div className="terminal-agent-session-list" aria-label={t("terminals.candidate_sessions")}>
          {loading && !current?.data ? <LoaderCircle className="spinning" /> : null}
          {!loading && !displayed.length ? <p>{t("terminals.no_matching_sessions_refresh_after_the_new_session_creates")}</p> : null}
          {displayed.map(session => <button type="button" key={session.id} onClick={() => choose(session)}>
            <strong><span>{session.title}</span><time dateTime={session.updated_at}>{relativeTime(session.updated_at)}</time></strong>
            <span><code>{session.session_id}</code><time dateTime={session.updated_at}>{new Date(session.updated_at).toLocaleString(getLanguage())}</time></span>
          </button>)}
        </div>
      </div>}
    </div> : null}
  </>;
}
