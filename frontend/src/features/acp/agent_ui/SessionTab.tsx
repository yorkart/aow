import '../../../components/conversation-theme.css';
// Conversation content is owned by a workspace tab; closing its view never closes the ACP session.
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Check, CircleAlert, GitFork, LogIn, MoreHorizontal, ScrollText, WifiOff } from 'lucide-react';
import { ConversationLayout } from '../../../components/conversation/ConversationLayout';
import { ConversationIds } from '../../../components/conversation/ConversationIds';
import { ComposerStatus } from '../../../components/conversation/ComposerStatus';
import { AgentIcon } from '../../agents/AgentIcon';
import { useAcpControl } from '../../acp-control/sessionControl';
import { ControlStatus } from '../../acp-control/ControlStatus';
import { Authentication } from './authentication';
import { Popover } from './popover';
import { ThinkingVisibility } from './thinking_visibility';
import { authenticationRequired } from '../request';
import { acpApi, authenticationCompleted, sessionsChanged } from '../api';
import { acceptSession, readSession } from '../sync/store';
import { useConnectionSync, useRequestsSync, useSessionSync } from '../sync/hooks';
import { failure, object, type ConnectionInfo, type Data, type DebugMessage, type SessionInfo, type SessionSnapshot } from '../types';
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
    // Recheck on an explicit control attachment before restoring the session.
    // A stale disconnected view must not reload another owner's live session.
    pending = readSession(id).then(next => next.status === 'disconnected' || next.needs_load
      ? acpApi.action(id, { action: 'resume' }, token) : next).finally(() => restores.delete(key));
    restores.set(key, pending);
  }
  return pending;
}

export function SessionTab({ tab, workspace, visible, onSessionChange, onOpenSession, onOpenFile }: {
  tab: AcpTab; workspace: string; visible: boolean;
  onSessionChange: (id: string, session: SessionInfo) => void; onOpenSession: (session: SessionInfo) => void; onOpenFile: OpenFile;
}) {
  const [connection, setConnection] = useState<ConnectionInfo>();
  const [prepared, setPrepared] = useState<SessionSnapshot>();
  const thread_id = prepared?.thread_id ?? tab.thread_id;
  const sync = useSessionSync(thread_id, visible);
  const session = sync.deleted ? undefined : sync.data;
  const binding = useConnectionSync(thread_id);
  const [busy, setBusy] = useState(true);
  const [busyMessage, setBusyMessage] = useState('正在打开会话…');
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState('');
  const [answerBusy, setAnswerBusy] = useState(false);
  const [logs, setLogs] = useState<DebugMessage[]>();
  const [showThinking, setShowThinking] = useState(false);
  // Request-only authentication belongs to the issuing control attachment; a
  // later owner may have authenticated without ever changing auth_required.
  const [requestAuthRequired, setRequestAuthRequired] = useState<{ token?: string }>();
  const [authBusy, setAuthBusy] = useState(false);
  const [authConnectionError, setAuthConnectionError] = useState('');
  const [authConnectionAttempt, setAuthConnectionAttempt] = useState(0);
  const recordFailure = useCallback((reason: unknown, token?: string) => { setError(failure(reason)); if (authenticationRequired(reason)) setRequestAuthRequired({ token }); }, []);
  const control = useAcpControl(thread_id, JSON.stringify([workspace, tab.id]));
  const token = control.token;
  const authRequired = (!thread_id || !!token) && (!!requestAuthRequired && requestAuthRequired.token === token || !!session?.auth_required || authBusy);
  const [restoring, setRestoring] = useState(false);
  const status = useProgress(connecting || restoring ? tab.agentId : '', workspace);
  const [restoreAttempt, setRestoreAttempt] = useState(0);
  const needsResume = !!session && (session.status === 'disconnected' || !!session.needs_load);
  const recovering = session?.recovery?.phase === 'loading' || session?.recovery?.phase === 'cleaning';
  const connectionId = connection?.id;
  const interactions = useRequestsSync(connectionId);
  const requests = (interactions.data ?? []).filter(item => !item.request.sessionId && !object(item.request.scope).sessionId);
  useEffect(() => { if (binding.data !== undefined) setConnection(binding.data ?? undefined); }, [binding.data]);
  useEffect(() => {
    const head = sync.head?.session;
    const info = head && (!session || head.generation !== session.sync_generation || head.revision >= session.revision) ? head.info : session;
    if (info) onSessionChange(tab.id, info);
  }, [sync.head, session, onSessionChange, tab.id]);
  const initialTab = useRef(tab).current;
  const [attempt, setAttempt] = useState(0);
  useEffect(() => { setError(''); }, [token]);
  const accept = useCallback((next: SessionSnapshot) => {
    acceptSession(next);
    setPrepared(next);
    onSessionChange(tab.id, next);
    window.dispatchEvent(new Event(sessionsChanged));
  }, [onSessionChange, tab.id]);
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
    if (initialTab.thread_id) { setBusy(false); return; }
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
  const resumeAttempts = useRef(new Map<string, Promise<SessionSnapshot> | undefined>());
  useEffect(() => {
    if (!thread_id || !token || !session || sync.deleted) return;
    const key = JSON.stringify([thread_id, token, restoreAttempt]);
    if (!needsResume) { resumeAttempts.current.set(key, undefined); setRestoring(false); return; }
    let pending = resumeAttempts.current.get(key);
    if (!pending && resumeAttempts.current.has(key)) { setRestoring(false); return; }
    if (!pending) { pending = resumeSession(thread_id, token); resumeAttempts.current.set(key, pending); }
    let cancelled = false;
    setRestoring(true); setError('');
    void pending
      .then(next => { if (!cancelled) accept(next); })
      .catch(reason => { if (!cancelled) recordFailure(reason, token); })
      .finally(() => { resumeAttempts.current.set(key, undefined); if (!cancelled) setRestoring(false); });
    return () => { cancelled = true; };
  }, [thread_id, token, needsResume, !!session, sync.deleted, restoreAttempt, accept, recordFailure]);
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
  const retry = () => { drafts.delete(initialTab); sync.reload(); setAttempt(value => value + 1); };
  const action = useCallback((value: Data) => {
    if (!thread_id || !token) return Promise.resolve(false);
    return run(async () => { accept(await acpApi.action(thread_id, value, token)); });
  }, [thread_id, token, run, accept]);
  useEffect(() => {
    if (!authRequired || connection) return;
    let cancelled = false;
    setAuthConnectionError('');
    void acpApi.connect(tab.agentId, workspace).then(value => { if (!cancelled) { setConnection(value); setAuthConnectionError(''); } }).catch(reason => { if (!cancelled) setAuthConnectionError(failure(reason)); });
    return () => { cancelled = true; };
  }, [authRequired, connection, tab.agentId, workspace, token, authConnectionAttempt]);
  const authenticate = (method: string) => {
    setAuthBusy(true); setError('');
    void (async () => {
      const next = await ensureConnection();
      await acpApi.authenticate(next.id, method);
      setRequestAuthRequired(undefined);
      if (thread_id) {
        const snapshot = await readSession(thread_id);
        accept(snapshot);
      } else retry();
    })().catch(reason => recordFailure(reason, token)).finally(() => setAuthBusy(false));
  };
  const headerActions = <><ConversationIds identifiers={[
    { label: 'Thread ID', value: thread_id },
    { label: 'Session ID', value: session?.session_id },
    { label: 'Connection ID', value: connectionId },
  ]} />{thread_id && <Popover label="会话更多操作" trigger={<MoreHorizontal size={18} />}>
    {close => <>
      <button type="button" role="menuitemcheckbox" aria-checked={showThinking} onClick={() => { setShowThinking(value => !value); close(); }}><Check size={14} style={{ visibility: showThinking ? 'visible' : 'hidden' }} />显示 Thinking</button>
      {!!object(connection?.capabilities?.sessionCapabilities).fork && <button type="button" role="menuitem" disabled={!token || busy || restoring || recovering || needsResume || session?.status !== 'idle'} onClick={() => {
        if (!token) return;
        close();
        void run(async () => {
          const fork = await acpApi.fork(thread_id, token);
          acceptSession(fork);
          window.dispatchEvent(new Event(sessionsChanged));
          onOpenSession(fork);
        });
      }}><GitFork size={14} />分叉会话</button>}
      <button type="button" role="menuitem" disabled={busy} onClick={() => { close(); if (logs) setLogs(undefined); else void run(async () => setLogs(await acpApi.logs(thread_id))); }}><ScrollText size={14} />{logs ? '返回会话' : 'ACP 协议日志'}</button>
    </>}
  </Popover>}</>;
  const connectionRequests = !authRequired && (!thread_id || token) && requests.length > 0 ? <div className="acp-content">{requests.map(request => <PermissionRequest key={request.id} permission={request} busy={answerBusy} onAnswer={response => { if (!connectionId) return; setAnswerBusy(true); void acpApi.answerConnection(connectionId, request.id, response).catch(recordFailure).finally(() => setAnswerBusy(false)); }} />)}</div> : undefined;
  // Show one blocking reason at the composer. Ownership takes precedence over
  // agent authentication and recovery, which only the controlling page can act on.
  let composerStatus: ReactNode;
  if (sync.deleted) {
    composerStatus = <ComposerStatus icon={<CircleAlert />} title="会话已删除。" description="此会话已不可用，可以关闭标签页并打开其他会话。" />;
  } else if (thread_id && sync.status === 'denied') {
    composerStatus = <ComposerStatus icon={<LogIn />} title="登录已失效，请重新登录。" description="重新登录网站后可继续使用。" actions={<button type="button" onClick={() => location.reload()}>重新登录</button>} />;
  } else if (thread_id && !token) {
    composerStatus = <ControlStatus {...control} />;
  } else if (authRequired) {
    composerStatus = connection ? <Authentication connection={connection} pending={authBusy} onAuthenticate={authenticate} composer error={error || session?.error || undefined} />
      : authConnectionError ? <ComposerStatus icon={<CircleAlert />} title="暂时无法连接登录服务" alert description={authConnectionError} actions={<button type="button" onClick={() => { setAuthConnectionError(''); setAuthConnectionAttempt(value => value + 1); }}>重试连接</button>} />
        : <ConnectionProgress composer message="正在准备 Agent 登录…" />;
  } else if (thread_id && (sync.status === 'offline' || sync.status === 'connecting')) {
    composerStatus = <ConnectionProgress composer message={sync.status === 'offline' ? '连接已断开，正在重新同步…' : '正在建立同步连接…'} />;
  } else if (!session) {
    composerStatus = busy || sync.loading ? <ConnectionProgress composer message={busyMessage} />
      : <ComposerStatus icon={<CircleAlert />} title="暂时无法打开会话" alert description={error || sync.error || '请重试打开会话。'} actions={<button type="button" onClick={retry}>重试打开会话</button>} />;
  } else if (restoring || recovering || connecting) {
    composerStatus = <ConnectionProgress composer status={session.recovery?.phase === 'cleaning' ? undefined : status}
      message={session.recovery?.phase === 'cleaning' ? '正在等待上次恢复结束并清理…' : session.session_id ? '正在恢复会话…' : '正在创建会话…'}
      actions={session.recovery?.phase === 'loading' && <button type="button" disabled={busy} onClick={() => { void action({ action: 'cancel' }); }}>取消恢复</button>}>{connectionRequests}</ConnectionProgress>;
  } else if (needsResume) {
    composerStatus = <ComposerStatus icon={<WifiOff />} title="暂时无法连接 Agent" alert
      description={session.recovery?.error ? `会话恢复失败：${session.recovery.error}` : error || session.error || '连接已断开，重试后可继续对话。'}
      actions={<button type="button" disabled={busy} onClick={() => setRestoreAttempt(value => value + 1)}>重试连接</button>}>{connectionRequests}</ComposerStatus>;
  } else if (sync.error) {
    composerStatus = <ComposerStatus icon={<CircleAlert />} title="暂时无法同步会话" alert description={sync.error} actions={<button type="button" onClick={sync.reload}>重试同步</button>} />;
  }
  const notice = <>
    {thread_id && sync.status === 'degraded' && <p className="acp-status" role="status">服务端暂不支持实时同步，已进入低频同步模式。</p>}
    {!composerStatus && <>{connectionRequests}
      {(binding.error || interactions.error) && <p className="acp-error" role="alert">{binding.error || interactions.error}</p>}
      {error && error !== session?.error && <p className="acp-error" role="alert">{error}</p>}
    </>}
  </>;
  const alternateContent = logs && thread_id ? <AcpLogs entries={logs} threadId={thread_id} agentName={tab.agentId} busy={busy} onBack={() => setLogs(undefined)} onRefresh={() => { void run(async () => setLogs(await acpApi.logs(thread_id))); }} /> : undefined;
  return <ThinkingVisibility.Provider value={showThinking}><section className="acp-session-tab aow-conversation-theme" aria-label="ACP 会话内容">
    {!session && <ConversationLayout sessionId={tab.id} label="ACP 对话" turns={[]}
      header={{ title: tab.title, agentName: tab.agentId, agentIcon: <AgentIcon agentId={tab.agentId.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} />, workspace, actions: headerActions }}
      alternateContent={alternateContent} footer={composerStatus}
    />}
    {session && <div className="acp-thread-host"><ThreadView session={session} workspace={workspace} tabId={tab.id} busy={busy || restoring || recovering} supportsImages={!!connection?.prompt_capabilities?.image} supportsSteering={object(connection?.extensions?.steering).supported === true} composerStatus={composerStatus} headerActions={headerActions} notice={notice} alternateContent={alternateContent} onAction={action} onOpenFile={onOpenFile} /></div>}
  </section></ThinkingVisibility.Provider>;
}
