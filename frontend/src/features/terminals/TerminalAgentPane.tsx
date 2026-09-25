import { appSessionStorage } from '../../lib/basePath';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { LoaderCircle, MessageSquare, RefreshCw, SquareTerminal } from 'lucide-react';
import type { AgentSessionSnapshot, AowAgentSession } from '../sessions/types';
import type { TerminalAgentProcess, TerminalPaneSessions } from './types';
import { terminalApi } from './terminalApi';
import { sessionsApi } from '../sessions/api';
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
  if (seconds === 0) return '刚刚';
  const ranges: [Intl.RelativeTimeFormatUnit, number][] = [
    ['year', 31_536_000], ['month', 2_592_000], ['week', 604_800],
    ['day', 86_400], ['hour', 3_600], ['minute', 60],
  ];
  const formatter = new Intl.RelativeTimeFormat('zh-CN', { numeric: 'always' });
  for (const [unit, size] of ranges) {
    if (Math.abs(seconds) >= size) return formatter.format(Math.round(seconds / size), unit);
  }
  return formatter.format(seconds, 'second');
}

export function TerminalAgentPane({ tabId, paneId, agentId, title, cwd, process, visible, header, terminal }: Props) {
  const supported = agentId === 'claude' || agentId === 'codex' || agentId === 'traecli' || agentId === 'hermes';
  const storageKey = `terminal.agent-session.${tabId}.${paneId}`;
  // Hermes titles come from its native database and can change after the first
  // turn or /title. They describe the session; they are not its identity.
  const identity = JSON.stringify([tabId, paneId, agentId, process?.pid, process?.start_time, process?.cwd ?? cwd,
    agentId === 'hermes' ? '' : terminalSessionTitle(title, process?.cwd ?? cwd)]);
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
        setError('Agent 进程已变化，请刷新会话列表');
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
      title={showing ? '返回终端' : '切换到会话详情'} aria-label={showing ? '返回终端' : '切换到会话详情'}
      aria-pressed={showing} onClick={() => setOpen(value => !value)}>
      {showing ? <SquareTerminal /> : <MessageSquare />}<span>{showing ? 'Terminal' : 'Conversation'}</span>
    </button> : null)}
    <div className="terminal-pane-surface" hidden={showing}>
      {terminal(visible && !showing)}
    </div>
    {showing ? <div className="terminal-agent-session-surface">
      <div className="terminal-agent-session-toolbar">
        <span title={selected?.session_id ?? current?.data?.cwd ?? cwd}>
          {selected ? `${current?.manual ? '已选择' : current?.data?.live_session_id ? '当前会话' : '标题匹配'} · ${selected.session_id}` : '选择当前会话'}
        </span>
        {selected ? <button type="button" onClick={reselect}>重新选择</button> : null}
        <button type="button" title="刷新会话列表" aria-label="刷新会话列表" disabled={loading} onClick={() => void refresh()}><RefreshCw className={loading ? 'spinning' : ''} /></button>
      </div>
      {error ? <div className="terminal-agent-session-error" role="alert">{error}</div> : null}
      {selected ? <SessionSnapshotView session={selected} snapshot={snapshot?.snapshot} loading={snapshot?.loading ?? true}
        actions={snapshot?.snapshot && <SessionShareButton session={selected} workspacePath={selected.cwd} />}
        error={snapshot?.error} onRefresh={() => void refresh()} /> : <div className="terminal-agent-session-picker">
        <p>{loading && !current?.data ? '正在查找当前会话…'
          : current?.data?.live_session_id && !matches.length ? '已识别当前会话，记录尚未就绪。可刷新重试或手动选择。'
          : matches.length ? `找到 ${matches.length} 个标题匹配的会话，请确认当前正在使用的会话。`
          : '未找到唯一匹配，请从当前目录的会话中选择。'}</p>
        <div className="terminal-agent-session-filter">
          <input aria-label="搜索会话标题或 ID" placeholder="搜索会话标题或 ID" value={query} onChange={event => setQuery(event.target.value)} />
          {matches.length > 0 ? <button type="button" onClick={() => setShowAll(value => !value)}>{showAll ? '只看标题匹配' : '显示全部会话'}</button> : null}
        </div>
        <div className="terminal-agent-session-directory" title={current?.data?.cwd ?? cwd}>{current?.data?.cwd ?? cwd}</div>
        <div className="terminal-agent-session-list" aria-label="候选会话">
          {loading && !current?.data ? <LoaderCircle className="spinning" /> : null}
          {!loading && !displayed.length ? <p>暂无匹配会话。新会话产生记录后可刷新重试。</p> : null}
          {displayed.map(session => <button type="button" key={session.id} onClick={() => choose(session)}>
            <strong><span>{session.title}</span><time dateTime={session.updated_at}>{relativeTime(session.updated_at)}</time></strong>
            <span><code>{session.session_id}</code><time dateTime={session.updated_at}>{new Date(session.updated_at).toLocaleString()}</time></span>
          </button>)}
        </div>
      </div>}
    </div> : null}
  </>;
}
