import { useCallback, useEffect, useState } from 'react';
import { resolveAcpSession } from '../session_location';
import { failure, type SessionInfo } from '../types';
import { SessionTab } from './SessionTab';
import type { AcpTab } from './workspace_tabs';

// A host without a workspace tab strip can open the same persisted conversation.
export function SessionPage({ workspace, thread_id, onOpenFile }: {
  workspace: string; thread_id: string; onOpenFile: (path: string, line?: number) => void;
}) {
  const [location, setLocation] = useState<{ workspace: string; thread_id: string; tab: AcpTab }>();
  const tab = location?.workspace === workspace && location.thread_id === thread_id ? location.tab : undefined;
  const [error, setError] = useState('');
  useEffect(() => {
    const controller = new AbortController();
    setLocation(undefined); setError('');
    void resolveAcpSession(thread_id, controller.signal).then(({ session, tabId }) => {
      if (session.cwd !== workspace) throw new Error('该 ACP 会话不属于此工作区。');
      if (!controller.signal.aborted) setLocation({ workspace, thread_id, tab: { id: tabId, thread_id: session.thread_id, agentId: session.agent_id, title: session.title } });
    }).catch(reason => { if (!controller.signal.aborted) setError(failure(reason)); });
    return () => controller.abort();
  }, [workspace, thread_id]);
  const changed = useCallback((_: string, session: SessionInfo) => {
    setLocation(current => current?.thread_id === session.thread_id && current.tab.title !== session.title ? { ...current, tab: { ...current.tab, title: session.title } } : current);
  }, []);
  return tab ? <SessionTab key={`${workspace}:${thread_id}`} tab={tab} workspace={workspace} visible onSessionChange={changed} onOpenSession={() => {}} onOpenFile={onOpenFile} />
    : <div className="aow-conversation-layout aow-conversation-theme"><p className={error ? 'acp-error' : 'acp-muted'} role={error ? 'alert' : 'status'}>{error || '正在打开会话…'}</p></div>;
}
