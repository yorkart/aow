import { t } from '../../../lib/i18n';
import '../../../components/conversation-theme.css';
// Conversation content is owned by a workspace tab; closing its view never closes the ACP session.
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Brain, CircleAlert, GitFork, LogIn, MessagesSquare, ScrollText, WifiOff } from 'lucide-react';
import { ConversationAction } from '../../../components/conversation/ConversationAction';
import { ConversationLayout } from '../../../components/conversation/ConversationLayout';
import { ConversationIds } from '../../../components/conversation/ConversationIds';
import { ComposerStatus } from '../../../components/conversation/ComposerStatus';
import { AgentIcon } from '../../agents/AgentIcon';
import { useAcpControl } from '../../acp-control/sessionControl';
import { ControlStatus } from '../../acp-control/ControlStatus';
import { Authentication } from './authentication';
import { ThinkingVisibility } from './thinking_visibility';
import { authenticationRequired } from '../request';
import { acpApi, authenticationCompleted, sessionsChanged } from '../api';
import { acceptSession, readSession } from '../sync/store';
import { useConnectionSync, useRequestsSync, useSessionSync } from '../sync/hooks';
import { failure, object, type ConnectionInfo, type Data, type DebugMessage, type SessionInfo, type SessionSnapshot } from '../types';
import { removeAcpSessionTabs, type AcpTab } from './workspace_tabs';
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
  useEffect(() => {
    if (thread_id && sync.deleted) removeAcpSessionTabs(workspace, thread_id);
  }, [workspace, thread_id, sync.deleted]);
  const binding = useConnectionSync(thread_id);
  const [busy, setBusy] = useState(true);
  const [busyMessage, setBusyMessage] = useState(t("acp.opening_session"));
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
      setBusyMessage(t("acp.creating_session"));
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
    setBusy(true); setError(''); setBusyMessage(t("acp.processing"));
    try { await operation(); return true; } catch (reason) { recordFailure(reason, token); return false; }
    finally { setBusy(false); setConnecting(false); }
  }, [recordFailure, token]);
  const ensureConnection = async () => {
    if (connection) return connection;
    setConnecting(true); setBusyMessage(t("acp.connecting_to_agent"));
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
    { label: t("navigation.thread_id"), value: thread_id },
    { label: t("navigation.session_id"), value: session?.session_id },
    { label: t("navigation.connection_id"), value: connectionId },
  ]} />{thread_id && <>
      <ConversationAction label={t("acp.show_thinking")} aria-pressed={showThinking} onClick={() => setShowThinking(value => !value)}><Brain size={16} /></ConversationAction>
      {!!object(connection?.capabilities?.sessionCapabilities).fork && <ConversationAction label={t("acp.fork_session")} disabled={!token || busy || restoring || recovering || needsResume || session?.status !== 'idle'} onClick={() => {
        if (!token) return;
        void run(async () => {
          const fork = await acpApi.fork(thread_id, token);
          acceptSession(fork);
          window.dispatchEvent(new Event(sessionsChanged));
          onOpenSession(fork);
        });
      }}><GitFork size={16} /></ConversationAction>}
      <ConversationAction label={logs ? t("acp.back_to_session") : t("acp.acp_protocol_logs")} disabled={busy} onClick={() => { if (logs) setLogs(undefined); else void run(async () => setLogs(await acpApi.logs(thread_id))); }}>{logs ? <MessagesSquare size={16} /> : <ScrollText size={16} />}</ConversationAction>
  </>}</>;
  const connectionRequests = !authRequired && (!thread_id || token) && requests.length > 0 ? <div className="acp-content">{requests.map(request => <PermissionRequest key={request.id} permission={request} busy={answerBusy} onAnswer={response => { if (!connectionId) return; setAnswerBusy(true); void acpApi.answerConnection(connectionId, request.id, response).catch(recordFailure).finally(() => setAnswerBusy(false)); }} />)}</div> : undefined;
  // Show one blocking reason at the composer. Ownership takes precedence over
  // agent authentication and recovery, which only the controlling page can act on.
  let composerStatus: ReactNode;
  if (sync.deleted) {
    composerStatus = <ComposerStatus icon={<CircleAlert />} title={t("acp.session_deleted")} description={t("acp.this_session_is_unavailable_close_the_tab_and_open")} />;
  } else if (thread_id && sync.status === 'denied') {
    composerStatus = <ComposerStatus icon={<LogIn />} title={t("acp.your_login_has_expired_sign_in_again")} description={t("acp.sign_in_to_the_website_again_to_continue")} actions={<button type="button" onClick={() => location.reload()}>{t("acp.sign_in_again")}</button>} />;
  } else if (thread_id && !token) {
    composerStatus = <ControlStatus {...control} />;
  } else if (authRequired) {
    composerStatus = connection ? <Authentication connection={connection} pending={authBusy} onAuthenticate={authenticate} composer error={error || session?.error || undefined} />
      : authConnectionError ? <ComposerStatus icon={<CircleAlert />} title={t("acp.unable_to_connect_to_the_login_service")} alert description={authConnectionError} actions={<button type="button" onClick={() => { setAuthConnectionError(''); setAuthConnectionAttempt(value => value + 1); }}>{t("acp.retry_connection")}</button>} />
        : <ConnectionProgress composer message={t("acp.preparing_agent_login")} />;
  } else if (thread_id && (sync.status === 'offline' || sync.status === 'connecting')) {
    composerStatus = <ConnectionProgress composer message={sync.status === 'offline' ? t("acp.disconnected_resyncing") : t("acp.establishing_sync_connection")} />;
  } else if (!session) {
    composerStatus = busy || sync.loading ? <ConnectionProgress composer message={busyMessage} />
      : <ComposerStatus icon={<CircleAlert />} title={t("acp.unable_to_open_session")} alert description={error || sync.error || t("acp.try_opening_the_session_again")} actions={<button type="button" onClick={retry}>{t("acp.retry_opening_session")}</button>} />;
  } else if (restoring || recovering || connecting) {
    composerStatus = <ConnectionProgress composer status={session.recovery?.phase === 'cleaning' ? undefined : status}
      message={session.recovery?.phase === 'cleaning' ? t("acp.waiting_for_the_previous_restoration_to_finish_and_clean") : session.session_id ? t("acp.restoring_session") : t("acp.creating_session")}
      actions={session.recovery?.phase === 'loading' && <button type="button" disabled={busy} onClick={() => { void action({ action: 'cancel' }); }}>{t("acp.cancel_restoration")}</button>}>{connectionRequests}</ConnectionProgress>;
  } else if (needsResume) {
    composerStatus = <ComposerStatus icon={<WifiOff />} title={t("acp.unable_to_connect_to_agent")} alert
      description={session.recovery?.error ? t("acp.session_restoration_failed", { value0: session.recovery.error }) : error || session.error || t("acp.disconnected_retry_to_continue_the_conversation")}
      actions={<button type="button" disabled={busy} onClick={() => setRestoreAttempt(value => value + 1)}>{t("acp.retry_connection")}</button>}>{connectionRequests}</ComposerStatus>;
  } else if (sync.error) {
    composerStatus = <ComposerStatus icon={<CircleAlert />} title={t("acp.unable_to_sync_session")} alert description={sync.error} actions={<button type="button" onClick={sync.reload}>{t("acp.retry_sync")}</button>} />;
  }
  const notice = <>
    {thread_id && sync.status === 'degraded' && <p className="acp-status" role="status">{t("acp.the_server_does_not_support_live_sync_using_periodic")}</p>}
    {!composerStatus && <>{connectionRequests}
      {(binding.error || interactions.error) && <p className="acp-error" role="alert">{binding.error || interactions.error}</p>}
      {error && error !== session?.error && <p className="acp-error" role="alert">{error}</p>}
    </>}
  </>;
  const alternateContent = logs && thread_id ? <AcpLogs entries={logs} threadId={thread_id} agentName={tab.agentId} busy={busy} onBack={() => setLogs(undefined)} onRefresh={() => { void run(async () => setLogs(await acpApi.logs(thread_id))); }} /> : undefined;
  return <ThinkingVisibility.Provider value={showThinking}><section className="acp-session-tab aow-conversation-theme" aria-label={t("acp.acp_session_content")}>
    {!session && <ConversationLayout sessionId={tab.id} label={t("acp.acp_conversation")} turns={[]}
      header={{ title: tab.title, agentName: tab.agentId, agentIcon: <AgentIcon agentId={tab.agentId.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} />, workspace, actions: headerActions }}
      alternateContent={alternateContent} footer={composerStatus}
    />}
    {session && <div className="acp-thread-host"><ThreadView session={session} workspace={workspace} tabId={tab.id} busy={busy || restoring || recovering} supportsImages={!!connection?.prompt_capabilities?.image} supportsSteering={object(connection?.extensions?.steering).supported === true} composerStatus={composerStatus} headerActions={headerActions} notice={notice} alternateContent={alternateContent} onAction={action} onOpenFile={onOpenFile} /></div>}
  </section></ThinkingVisibility.Provider>;
}
