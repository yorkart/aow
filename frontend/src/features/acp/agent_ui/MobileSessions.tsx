import { ChevronRight, List, LoaderCircle } from 'lucide-react';
import { createPortal } from 'react-dom';
import { TaskCompletion } from '../../../components/TaskCompletion';
import { MobilePageHeader, MobileRefresh, MobileState, mobileTime } from '../../../mobile/MobilePrimitives';
import { useMobileResource, useMobileScroll, type MobileNavigate, type MobileRoute } from '../../../mobile/mobileState';
import { AgentIcon } from '../../agents/AgentIcon';
import { useUnreadAcpThreadIds } from '../../notifications/taskNotifications';
import { acpApi } from '../api';
import { SessionPage } from './SessionPage';

export function MobileSessions({ workspace, route, visible, navigate, headerActions }: {
  workspace: string; route: MobileRoute; visible: boolean; navigate: MobileNavigate; headerActions: HTMLElement | null;
}) {
  const showList = visible && !route.acpSession;
  const key = `acp.sessions.${workspace}`;
  const list = useMobileResource(key, () => acpApi.sessions(workspace), showList);
  const scroll = useMobileScroll(key, showList && !!list.data);
  const unread = useUnreadAcpThreadIds();
  const sessions = list.data?.filter(session => session.cwd === workspace)
    .sort((a, b) => (Date.parse(b.updated_at) || 0) - (Date.parse(a.updated_at) || 0));
  if (visible && route.acpSession) return <>
    {headerActions && createPortal(<button className="mobile-icon-button" aria-label="返回 ACP 会话列表" title="返回 ACP 会话列表"
      onClick={() => navigate({ workspace, view: 'acp' }, true)}><List size={20} /></button>, headerActions)}
    <SessionPage workspace={workspace} thread_id={route.acpSession} onOpenFile={path => navigate({ workspace, view: 'files', file: path, fileSource: 'external' })} />
  </>;
  return <section className="mobile-content-page" aria-label="ACP 会话列表">
    <MobilePageHeader title="ACP" subtitle="本地会话记录" actions={<MobileRefresh reload={list.reload} loading={list.loading} />} />
    <div className="mobile-scroll" ref={scroll}>
      <MobileState error={list.error} loading={list.loading && !list.data} retry={list.reload} empty={sessions?.length === 0 ? '这个工作区还没有 ACP 会话记录。' : undefined} />
      <div className="mobile-list">{sessions?.map(session => <button className="mobile-list-row mobile-session-row" key={session.thread_id}
        aria-label={`打开 ACP 会话 ${session.title}`} onClick={() => navigate({ workspace, view: 'acp', acpSession: session.thread_id })}>
        <div className="mobile-row-icon"><AgentIcon agentId={session.agent_id.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} /></div>
        <div className="mobile-row-main"><strong>{session.title}</strong><span>{session.agent_id}{session.updated_at && ` · ${mobileTime(session.updated_at)}`}</span></div>
        {session.status === 'working' && <LoaderCircle className="spinning" size={16} aria-label="执行中" />}
        {unread.has(session.thread_id) && <TaskCompletion />}
        <ChevronRight size={17} />
      </button>)}</div>
    </div>
  </section>;
}
