import '../../../components/conversation-theme.css';
// Conversation content is owned by a workspace tab; closing its view never closes the ACP session.
import { useCallback, useEffect, useRef, useState } from 'react';
import { MoreHorizontal, ScrollText } from 'lucide-react';
import { ConversationLayout } from '../../../components/conversation/ConversationLayout';
import { AgentIcon } from '../../agents/AgentIcon';
import { useAcpControl } from '../../acp-control/sessionControl';
import { ControlStatus } from '../../acp-control/ControlStatus';
import { Authentication } from './authentication';
import { Popover } from './popover';
import { authenticationRequired } from '../request';
import { acpApi, authenticationCompleted, sessionsChanged } from '../api';
import { applySessionUpdates } from '../session_updates';
import { failure, object, type ConnectionInfo, type Data, type DebugMessage, type Permission, type SessionInfo, type SessionSnapshot } from '../types';
import type { AcpTab } from './workspace_tabs';
import { ConnectionProgress } from './connection_progress';
import { useProgress } from './use_progress';
import { PermissionRequest } from './conversation_view/elicitation';
import { ThreadView } from './conversation_view/thread_view';
import { AcpLogs } from '../acp_tools';
import type { OpenFile } from './conversation_view/content';
import '../acp.css';

// A tab can move between portal hosts while its initial request is still pending.
// Keep that request with the draft descriptor so remounting cannot create another session.
const drafts = new WeakMap<AcpTab, Promise<SessionSnapshot>>();

// Share an owner's restore across portal remounts; a different owner has a new token.
const restores = new Map<string, Promise<SessionSnapshot>>();
function resumeSession(id: string, token: string) {
  const key = JSON.stringify([id, token]);
  let pending = restores.get(key);
  if (!pending) {
    // A takeover may happen before this view's next poll. Recheck the snapshot
    // so a stale disconnected view never reloads another owner's live session.
    pending = acpApi.snapshot(id).then(next => next.status === 'disconnected' || next.needs_load
      ? acpApi.action(id, { action: 'resume' }, token) : next).finally(() => restores.delete(key));
    restores.set(key, pending);
  }
  return pending;
}

export function SessionTab({ tab, workspace, visible, onSessionChange, onOpenFile }: {
  tab: AcpTab; workspace: string; visible: boolean;
  onSessionChange: (id: string, session: SessionInfo) => void; onOpenSession: (session: SessionInfo) => void; onOpenFile: OpenFile;
}) {
  const [connection, setConnection] = useState<ConnectionInfo>();
  const [session, setSession] = useState<SessionSnapshot>();
  const sessionRef = useRef<SessionSnapshot>(undefined);
  const [busy, setBusy] = useState(true);
  const [busyMessage, setBusyMessage] = useState('正在打开会话…');
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState('');
  const [requests, setRequests] = useState<Permission[]>([]);
  const [answerBusy, setAnswerBusy] = useState(false);
  const [logs, setLogs] = useState<DebugMessage[]>();
  // Request-only authentication belongs to the issuing control attachment; a
  // later owner may have authenticated without ever changing auth_required.
  const [requestAuthRequired, setRequestAuthRequired] = useState<{ token?: string }>();
  const [authBusy, setAuthBusy] = useState(false);
  const recordFailure = useCallback((reason: unknown, token?: string) => { setError(failure(reason)); if (authenticationRequired(reason)) setRequestAuthRequired({ token }); }, []);
  const thread_id = session?.thread_id ?? tab.thread_id;
  const control = useAcpControl(thread_id, JSON.stringify([workspace, tab.id]));
  const token = control.token;
  const authRequired = (!thread_id || !!token) && (!!requestAuthRequired && requestAuthRequired.token === token || !!session?.auth_required || authBusy);
  const [restoring, setRestoring] = useState(false);
  const status = useProgress(connecting || restoring ? tab.agentId : '', workspace);
  const [restoreAttempt, setRestoreAttempt] = useState(0);
  const needsResume = !!session && (session.status === 'disconnected' || !!session.needs_load);
  const polling = visible || session?.status === 'working';
  const connectionId = connection?.id;
  const initialTab = useRef(tab).current;
  const [attempt, setAttempt] = useState(0);
  useEffect(() => { setError(''); }, [token]);
  const acceptSnapshot = useCallback((next: SessionSnapshot) => {
    const previous = sessionRef.current;
    if (previous?.thread_id === next.thread_id && previous.sync_generation === next.sync_generation) {
      if (previous.revision > next.revision) return;
      // A same-revision overview still releases a previously accepted full body.
      if (previous.revision === next.revision && (previous.overview || !next.overview)) return;
    }
    sessionRef.current = next;
    setSession(next);
  }, []);
  const accept = useCallback((next: SessionSnapshot) => {
    acceptSnapshot(next);
    onSessionChange(tab.id, next);
    window.dispatchEvent(new Event(sessionsChanged));
  }, [acceptSnapshot, onSessionChange, tab.id]);
  useEffect(() => {
    // Once the backend records AuthRequired it owns subsequent true -> false transitions.
    if (session?.auth_required) setRequestAuthRequired(undefined);
  }, [session?.auth_required, requestAuthRequired]);
  useEffect(() => {
    const authenticated = (event: Event) => {
      if ((event as CustomEvent<string>).detail === connectionId) { setRequestAuthRequired(undefined); setError(''); setRestoreAttempt(value => value + 1); }
    };
    window.addEventListener(authenticationCompleted, authenticated);
    return () => window.removeEventListener(authenticationCompleted, authenticated);
  }, [connectionId]);
  useEffect(() => {
    let cancelled = false;
    setBusy(true); setError('');
    let pending: Promise<SessionSnapshot>;
    if (initialTab.thread_id) pending = acpApi.snapshot(initialTab.thread_id);
    else {
      setBusyMessage('正在创建会话…');
      let prepared = drafts.get(initialTab);
      if (!prepared) {
        prepared = acpApi.create(initialTab.agentId, workspace);
        drafts.set(initialTab, prepared);
      }
      pending = prepared;
    }
    void pending.then(next => { if (!cancelled) accept(next); })
      .catch(reason => { if (!cancelled) recordFailure(reason); })
      .finally(() => { if (!cancelled) { setConnecting(false); setBusy(false); } });
    return () => { cancelled = true; };
  }, [accept, initialTab, workspace, attempt, recordFailure]);
  useEffect(() => {
    if (!thread_id || !token || !needsResume) { setRestoring(false); return; }
    let cancelled = false;
    setRestoring(true); setError('');
    void resumeSession(thread_id, token)
      .then(next => { if (!cancelled) accept(next); })
      .catch(reason => { if (!cancelled) recordFailure(reason, token); })
      .finally(() => { if (!cancelled) setRestoring(false); });
    return () => { cancelled = true; };
  }, [thread_id, token, needsResume, restoreAttempt, accept, recordFailure]);
  useEffect(() => {
    if (!polling || !thread_id && !connectionId) return;
    const controller = new AbortController(); let timer: number;
    const poll = async () => {
      try {
        if (thread_id) {
          const update = await acpApi.updates(thread_id, sessionRef.current, controller.signal);
          if (controller.signal.aborted) return;
          const next = applySessionUpdates(sessionRef.current, update) ?? await acpApi.snapshot(thread_id, controller.signal);
          if (!controller.signal.aborted && next !== sessionRef.current) { acceptSnapshot(next); onSessionChange(tab.id, next); }
        }
        if (thread_id && token) {
          const connected = await acpApi.connection(thread_id, controller.signal);
          if (!controller.signal.aborted) setConnection(previous => previous?.id === connected?.id ? previous : connected ?? undefined);
        }
        if (connectionId) { const pending = await acpApi.requests(connectionId, controller.signal); if (!controller.signal.aborted) setRequests(pending.filter(item => !item.request.sessionId && !object(item.request.scope).sessionId)); }
      } catch (reason) { if (!controller.signal.aborted) setError(failure(reason)); }
      if (!controller.signal.aborted) timer = window.setTimeout(() => { void poll(); }, 750);
    };
    void poll();
    return () => { controller.abort(); window.clearTimeout(timer); };
  }, [polling, thread_id, connectionId, token, acceptSnapshot, onSessionChange, tab.id]);
  const run = useCallback(async (operation: () => Promise<void>) => {
    setBusy(true); setError(''); setBusyMessage('处理中…');
    try { await operation(); return true; } catch (reason) { recordFailure(reason, token); return false; }
    finally { setBusy(false); setConnecting(false); }
  }, [recordFailure, token]);
  const ensureConnection = async () => {
    if (connection) return connection;
    setConnecting(true); setBusyMessage('正在连接 Agent…');
    const next = await acpApi.connect(tab.agentId, workspace); setConnection(next); setConnecting(false); return next;
  };
  const retry = () => { drafts.delete(initialTab); setAttempt(value => value + 1); };
  const action = useCallback((value: Data) => {
    if (!thread_id || !token) return Promise.resolve(false);
    return run(async () => { accept(await acpApi.action(thread_id, value, token)); });
  }, [thread_id, token, run, accept]);
  useEffect(() => {
    if (!authRequired || connection) return;
    let cancelled = false;
    void acpApi.connect(tab.agentId, workspace).then(value => { if (!cancelled) setConnection(value); }).catch(reason => { if (!cancelled) recordFailure(reason, token); });
    return () => { cancelled = true; };
  }, [authRequired, connection, tab.agentId, workspace, recordFailure, token]);
  const authenticate = (method: string) => {
    setAuthBusy(true); setError('');
    void (async () => {
      const next = await ensureConnection();
      await acpApi.authenticate(next.id, method);
      setRequestAuthRequired(undefined);
      if (thread_id) {
        const snapshot = await acpApi.snapshot(thread_id);
        accept(snapshot);
      } else retry();
    })().catch(reason => recordFailure(reason, token)).finally(() => setAuthBusy(false));
  };
  const threadHidden = !!logs || authRequired;
  const headerActions = thread_id && <Popover label="会话更多操作" trigger={<MoreHorizontal size={18} />}>
    {close => <button type="button" role="menuitem" disabled={busy} onClick={() => { close(); if (logs) setLogs(undefined); else void run(async () => setLogs(await acpApi.logs(thread_id))); }}><ScrollText size={14} />{logs ? '返回会话' : 'ACP 协议日志'}</button>}
  </Popover>;
  const connectionState = <div className="acp-content">
    {error && error !== session?.error && !authRequired && <p className="acp-error" role="alert">{error}</p>}
    {token && restoring && !authRequired && <ConnectionProgress status={status} message={session?.session_id ? '正在恢复会话…' : '正在创建会话…'} />}
    {token && needsResume && !restoring && !busy && !authRequired && <button type="button" onClick={() => setRestoreAttempt(value => value + 1)}>重试连接</button>}
    {busy && (!session || session.needs_load || connecting) && !authRequired && <ConnectionProgress status={connecting ? status : undefined} message={busyMessage} />}
    {!authBusy && (!thread_id || token) && requests.map(request => <PermissionRequest key={request.id} permission={request} busy={answerBusy} onAnswer={response => { if (!connectionId) return; setAnswerBusy(true); void acpApi.answerConnection(connectionId, request.id, response).catch(recordFailure).finally(() => setAnswerBusy(false)); }} />)}
  </div>;
  const alternateContent = threadHidden ? <>
    {logs && thread_id && <AcpLogs entries={logs} threadId={thread_id} agentName={tab.agentId} busy={busy} onBack={() => setLogs(undefined)} onRefresh={() => { void run(async () => setLogs(await acpApi.logs(thread_id))); }} />}
    {authRequired && connection && <div className="acp-content acp-auth-state">{error && <p className="acp-error" role="alert">{error}</p>}<Authentication connection={connection} pending={authBusy} onAuthenticate={authenticate} /></div>}
  </> : undefined;
  return <section className="acp-session-tab aow-conversation-theme" aria-label="ACP 会话内容">
    {!session && <ConversationLayout sessionId={tab.id} label="ACP 对话" turns={[]}
      header={{ title: tab.title, agentName: tab.agentId, agentIcon: <AgentIcon agentId={tab.agentId.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} />, workspace, actions: headerActions }}
      banner={connectionState} alternateContent={alternateContent}
      state={!busy && !authRequired ? <div className="acp-content acp-empty"><button type="button" onClick={retry}>重试打开会话</button></div> : undefined}
    />}
    {session && <div className="acp-thread-host"><ThreadView session={session} workspace={workspace} tabId={tab.id} busy={busy || restoring} supportsImages={!!connection?.prompt_capabilities?.image} readOnly={!token} controlStatus={<ControlStatus {...control} />} headerActions={headerActions} banner={connectionState} alternateContent={alternateContent} onAction={action} onOpenFile={onOpenFile} /></div>}
  </section>;
}
