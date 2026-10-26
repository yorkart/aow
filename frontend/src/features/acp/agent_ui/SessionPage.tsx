import { t } from '../../../lib/i18n';
import { useCallback, useEffect, useState } from 'react';
import { resolveAcpSession } from '../session_location';
import { failure, type SessionInfo } from '../types';
import { MobileConversationHeader } from '../../../mobile/MobileConversationHeader';
import { SessionTab } from './SessionTab';
import type { AcpTab } from './workspace_tabs';

// A host without a workspace tab strip can open the same persisted conversation.
export function SessionPage({ workspace, thread_id, onOpenSession, onOpenFile, onBack }: {
  workspace: string; thread_id: string; onOpenFile: (path: string, line?: number) => void;
  onOpenSession: (session: SessionInfo) => void; onBack?: () => void;
}) {
  const [location, setLocation] = useState<{ workspace: string; thread_id: string; tab: AcpTab }>();
  const tab = location?.workspace === workspace && location.thread_id === thread_id ? location.tab : undefined;
  const [error, setError] = useState('');
  useEffect(() => {
    const controller = new AbortController();
    setLocation(undefined); setError('');
    void resolveAcpSession(thread_id, controller.signal).then(({ session, tabId }) => {
      if (session.cwd !== workspace) throw new Error(t("acp.this_acp_session_belongs_to_a_different_workspace"));
      if (!controller.signal.aborted) setLocation({ workspace, thread_id, tab: { id: tabId, thread_id: session.thread_id, agentId: session.agent_id, title: session.title } });
    }).catch(reason => { if (!controller.signal.aborted) setError(failure(reason)); });
    return () => controller.abort();
  }, [workspace, thread_id]);
  const changed = useCallback((_: string, session: SessionInfo) => {
    setLocation(current => current?.thread_id === session.thread_id && current.tab.title !== session.title ? { ...current, tab: { ...current.tab, title: session.title } } : current);
  }, []);
  return tab ? <SessionTab key={`${workspace}:${thread_id}`} tab={tab} workspace={workspace} visible onSessionChange={changed} onOpenSession={onOpenSession} onOpenFile={onOpenFile} onBack={onBack} />
    : <div className="aow-conversation-layout aow-conversation-theme">
      {onBack && <MobileConversationHeader title="ACP" back={onBack} backLabel={t("acp.back_to_acp_sessions")} identifiers={[
        { label: t("navigation.thread_id"), value: thread_id }, { label: t("automations.workspace"), value: workspace },
      ]} />}
      <p className={error ? 'acp-error' : 'acp-muted'} role={error ? 'alert' : 'status'}>{error || t("acp.opening_session")}</p></div>;
}
