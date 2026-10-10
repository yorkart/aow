// Conversation content is owned by a workspace tab; closing its view never closes the ACP session.
import { useCallback, useEffect, useRef, useState } from 'react';
import { Bot, MoreHorizontal, ScrollText } from 'lucide-react';
import { Authentication } from './authentication';
import { Popover } from './popover';
import { authenticationRequired } from '../request';
import { acpApi, authenticationCompleted, sessionsChanged } from '../api';
import { failure, object, type ConnectionInfo, type Data, type DebugMessage, type Permission, type SessionInfo, type SessionSnapshot } from '../types';
import type { AcpTab } from './workspace_tabs';
import { ConnectionProgress } from './connection_progress';
import { useProgress } from './use_progress';
import { PermissionRequest } from './conversation_view/elicitation';
import { ThreadView } from './conversation_view/thread_view';
import type { OpenFile } from './conversation_view/content';
import '../zed.css';

// A tab can move between portal hosts while its initial request is still pending.
// Keep that request with the draft descriptor so remounting cannot create another session.
const drafts = new WeakMap<AcpTab, { connection: Promise<ConnectionInfo>; session: Promise<SessionSnapshot> }>();

export function SessionTab({ tab, workspace, visible, onSessionChange, onOpenFile }: {
  tab: AcpTab; workspace: string; visible: boolean;
  onSessionChange: (id: string, session: SessionInfo) => void; onOpenSession: (session: SessionInfo) => void; onOpenFile: OpenFile;
}) {
  const [connection, setConnection] = useState<ConnectionInfo>();
  const [session, setSession] = useState<SessionSnapshot>();
  const [busy, setBusy] = useState(true);
  const [busyMessage, setBusyMessage] = useState('正在打开会话…');
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState('');
  const [requests, setRequests] = useState<Permission[]>([]);
  const [answerBusy, setAnswerBusy] = useState(false);
  const [logs, setLogs] = useState<DebugMessage[]>();
  // A failed new/load/resume request need not change the backend snapshot.
  const [requestAuthRequired, setRequestAuthRequired] = useState(false);
  const [authBusy, setAuthBusy] = useState(false);
  const authRequired = requestAuthRequired || !!session?.auth_required || authBusy;
  const recordFailure = useCallback((reason: unknown) => { setError(failure(reason)); if (authenticationRequired(reason)) setRequestAuthRequired(true); }, []);
  const status = useProgress(connecting ? tab.agentId : '', workspace);
  const sessionId = session?.id ?? tab.sessionId;
  const connectionId = connection?.id;
  const initialTab = useRef(tab).current;
  const retryConnection = useRef<ConnectionInfo | undefined>(undefined);
  const [attempt, setAttempt] = useState(0);
  const acceptSnapshot = useCallback((next: SessionSnapshot) => {
    setSession(previous => previous?.id === next.id && previous.revision >= next.revision ? previous : next);
  }, []);
  const accept = useCallback((next: SessionSnapshot) => {
    acceptSnapshot(next);
    onSessionChange(tab.id, next);
    window.dispatchEvent(new Event(sessionsChanged));
  }, [acceptSnapshot, onSessionChange, tab.id]);
  useEffect(() => {
    // Once the backend records AuthRequired it owns subsequent true -> false transitions.
    if (session?.auth_required) setRequestAuthRequired(false);
  }, [session?.auth_required, requestAuthRequired]);
  useEffect(() => {
    const authenticated = (event: Event) => {
      if ((event as CustomEvent<string>).detail === connectionId) { setRequestAuthRequired(false); setError(''); }
    };
    window.addEventListener(authenticationCompleted, authenticated);
    return () => window.removeEventListener(authenticationCompleted, authenticated);
  }, [connectionId]);
  useEffect(() => {
    let cancelled = false;
    setBusy(true); setError('');
    let pending: Promise<SessionSnapshot>;
    if (initialTab.sessionId) pending = acpApi.snapshot(initialTab.sessionId);
    else {
      setConnecting(true); setBusyMessage('正在连接 Agent…');
      let opening = drafts.get(initialTab);
      if (!opening) {
        const connected = retryConnection.current ? Promise.resolve(retryConnection.current) : acpApi.connect(initialTab.agentId, workspace);
        opening = { connection: connected, session: connected.then(value => acpApi.create(value.id)) };
        drafts.set(initialTab, opening);
      }
      void opening.connection.then(connected => {
        if (!cancelled) { setConnection(connected); setConnecting(false); setBusyMessage('正在创建会话…'); }
      }).catch(() => { /* The session promise reports connection failures below. */ });
      pending = opening.session;
    }
    void pending.then(next => { if (!cancelled) accept(next); })
      .catch(reason => { if (!cancelled) recordFailure(reason); })
      .finally(() => { if (!cancelled) { setConnecting(false); setBusy(false); } });
    return () => { cancelled = true; };
  }, [accept, initialTab, workspace, attempt, recordFailure]);
  useEffect(() => {
    if (!visible || !sessionId && !connectionId) return;
    const controller = new AbortController(); let timer: number;
    const poll = async () => {
      try {
        if (sessionId) { const next = await acpApi.snapshot(sessionId, controller.signal); if (!controller.signal.aborted) { acceptSnapshot(next); onSessionChange(tab.id, next); } }
        if (connectionId) { const pending = await acpApi.requests(connectionId, controller.signal); if (!controller.signal.aborted) setRequests(pending.filter(item => !item.request.sessionId && !object(item.request.scope).sessionId)); }
      } catch (reason) { if (!controller.signal.aborted) setError(failure(reason)); }
      if (!controller.signal.aborted) timer = window.setTimeout(() => { void poll(); }, 750);
    };
    void poll();
    return () => { controller.abort(); window.clearTimeout(timer); };
  }, [visible, sessionId, connectionId, acceptSnapshot, onSessionChange, tab.id]);
  const run = async (operation: () => Promise<void>) => {
    setBusy(true); setError(''); setBusyMessage('处理中…');
    try { await operation(); return true; } catch (reason) { recordFailure(reason); return false; }
    finally { setBusy(false); setConnecting(false); }
  };
  const ensureConnection = async () => {
    if (connection) return connection;
    setConnecting(true); setBusyMessage('正在连接 Agent…');
    const next = await acpApi.connect(tab.agentId, workspace); setConnection(next); setConnecting(false); return next;
  };
  const retry = () => { retryConnection.current = connection; drafts.delete(initialTab); setAttempt(value => value + 1); };
  const action = (value: Data) => run(async () => { if (sessionId) accept(await acpApi.action(sessionId, value)); });
  useEffect(() => {
    if (!authRequired || connection) return;
    let cancelled = false;
    void acpApi.connect(tab.agentId, workspace).then(value => { if (!cancelled) setConnection(value); }).catch(reason => { if (!cancelled) recordFailure(reason); });
    return () => { cancelled = true; };
  }, [authRequired, connection, tab.agentId, workspace, recordFailure]);
  const authenticate = (method: string) => {
    setAuthBusy(true); setError('');
    void (async () => {
      const next = await ensureConnection();
      await acpApi.authenticate(next.id, method);
      setRequestAuthRequired(false);
      if (sessionId) accept(await acpApi.snapshot(sessionId)); else retry();
    })().catch(recordFailure).finally(() => setAuthBusy(false));
  };
  return <section className="zed-panel zed-session-tab" aria-label="ACP 会话内容">
    <header className="zed-header zed-conversation-header"><Bot size={17} /><strong>{session?.title ?? tab.title}</strong><span />
      {sessionId && <Popover label="会话更多操作" trigger={<MoreHorizontal size={18} />}>
        {close => <button type="button" role="menuitem" disabled={busy} onClick={() => { close(); if (logs) setLogs(undefined); else void run(async () => setLogs(await acpApi.logs(sessionId))); }}><ScrollText size={14} />{logs ? '返回会话' : 'ACP 协议日志'}</button>}
      </Popover>}
    </header>
    {error && !authRequired && <p className="zed-error" role="alert">{error}</p>}
    {busy && (!session || connecting) && !authRequired && <ConnectionProgress status={connecting ? status : undefined} message={busyMessage} />}
    {!authBusy && requests.map(request => <PermissionRequest key={request.id} permission={request} busy={answerBusy} onAnswer={response => { if (!connectionId) return; setAnswerBusy(true); void acpApi.answerConnection(connectionId, request.id, response).catch(recordFailure).finally(() => setAnswerBusy(false)); }} />)}
    {logs && <div className="zed-logs"><button type="button" onClick={() => setLogs(undefined)}>返回会话</button>{logs.map((entry, index) => <details key={index}><summary>{entry.timestamp} {entry.direction}</summary><pre>{entry.message}</pre></details>)}</div>}
    {session && <div className="zed-thread-host" hidden={!!logs || authRequired}><ThreadView session={session} workspace={workspace} tabId={tab.id} busy={busy} onAction={action} onOpenFile={onOpenFile} /></div>}
    {authRequired && connection && <div className="zed-auth-state">{error && <p className="zed-error" role="alert">{error}</p>}<Authentication connection={connection} pending={authBusy} onAuthenticate={authenticate} /></div>}
    {!session && !busy && !authRequired && <div className="zed-empty"><button type="button" onClick={retry}>重试打开会话</button></div>}
  </section>;
}
