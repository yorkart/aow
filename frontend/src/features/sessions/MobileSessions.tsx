import { ChevronRight, MessageSquare } from 'lucide-react';
import { sessionsApi } from './api';
import { AgentIcon } from '../agents/AgentIcon';
import { SessionShareButton } from './SessionShareButton';
import { SessionMessageContent } from './SessionMessageContent';
import { SessionTurnPending, SessionTurnProcess } from './SessionTurnProcess';
import type { AowAgentSession } from './types';
import { MobilePageHeader, MobileRefresh, MobileState, mobileTime } from '../../mobile/MobilePrimitives';
import { useMobileResource, useMobileScroll, type MobileRoute, type MobileNavigate } from '../../mobile/mobileState';

const agents = ['codex', 'claude', 'traecli', 'hermes'] as const;
const statusNames = { completed: '已完成', failed: '失败', interrupted: '已中断', in_progress: '进行中' };

export function MobileSessions({ route, visible, navigate, back }: { route: MobileRoute; visible: boolean; navigate: MobileNavigate; back: () => void }) {
  const list = useMobileResource(`sessions.${route.cwd ?? route.workspace}`, async () => {
    const results = await Promise.allSettled(agents.map((agent) => sessionsApi.agentSessions(route.cwd ?? route.workspace, agent)));
    const sessions = results.flatMap((result) => result.status === 'fulfilled' ? result.value : []);
    const failures = results.flatMap((result, index) => result.status === 'rejected' ? [`${agents[index]}: ${String(result.reason)}`] : []);
    if (failures.length === agents.length) throw new Error(failures.join('\n'));
    return { sessions: sessions.sort((a, b) => b.updated_at.localeCompare(a.updated_at)), failures };
  }, visible);
  const scroll = useMobileScroll(`sessions.${route.cwd ?? route.workspace}`, !!list.data);
  const selected = list.data?.sessions.find((session) => session.id === route.session || session.agent === route.agent && session.agent + ':' + session.session_id === route.session);
  return <section className="mobile-content-page">
    {route.session ? selected ? <MobileSessionReader session={selected} workspace={route.cwd ?? selected.cwd} back={back} />
      : <><MobilePageHeader title="会话" back={back} /><MobileState loading={list.loading} error={list.error} retry={list.reload} empty="未找到这个会话，请返回列表刷新。" /></>
      : <>
        <MobilePageHeader title="Conversation" subtitle="本地会话记录" actions={<MobileRefresh reload={list.reload} loading={list.loading} />} />
        <div className="mobile-scroll" ref={scroll}>
          <MobileState error={list.error} loading={list.loading && !list.data} retry={list.reload} empty={list.data?.sessions.length === 0 ? '这个工作区还没有会话记录。' : undefined} />
          {list.data?.failures.map((failure) => <p className="mobile-inline-error" key={failure}>{failure}</p>)}
          <div className="mobile-list">{list.data?.sessions.map((session) => <button className="mobile-list-row mobile-session-row" key={session.id}
            onClick={() => navigate({ workspace: route.workspace, view: 'sessions', session: session.id, agent: session.agent, cwd: session.cwd })}>
            <div className="mobile-row-icon"><AgentIcon agentId={session.agent} /></div>
            <div className="mobile-row-main"><strong>{session.title}</strong><span>{session.agent} · {mobileTime(session.updated_at)}</span></div><ChevronRight size={17} />
          </button>)}</div>
        </div>
      </>}
  </section>;
}

export function MobileSessionReader({ session, workspace, back }: { session: AowAgentSession; workspace: string; back: () => void }) {
  const snapshot = useMobileResource(`snapshot.${workspace}.${session.id}`, () => sessionsApi.agentSessionSnapshot(session, workspace));
  const scroll = useMobileScroll(`snapshot.${workspace}.${session.id}`, !!snapshot.data);
  return <section className="mobile-content-page">
    <MobilePageHeader title={session.title} subtitle={snapshot.data ? `${session.agent} · ${statusNames[snapshot.data.status]} · ${snapshot.data.turns.length} 轮` : session.agent}
      back={back} actions={<>{snapshot.data && <SessionShareButton session={session} workspacePath={workspace} />}<MobileRefresh reload={snapshot.reload} loading={snapshot.loading} /></>} />
    <div className="mobile-scroll mobile-conversation" ref={scroll}>
      <MobileState error={snapshot.error} loading={snapshot.loading && !snapshot.data} retry={snapshot.reload} empty={snapshot.data?.turns.length === 0 ? '这个会话还没有可展示的对话。' : undefined} />
      {snapshot.data && <p className="mobile-reader-meta"><MessageSquare size={13} />快照更新于 {mobileTime(snapshot.data.captured_at)}{snapshot.data.truncated && ' · 仅展示最近 200 轮'}</p>}
      {snapshot.data?.turns.map((turn, index) => <article className="mobile-turn" key={turn.id}>
        <div className="mobile-turn-label">第 {index + 1} 轮 · {statusNames[turn.status]}{index === snapshot.data!.turns.length - 1 && <span>最新一轮</span>}</div>
        <section className="mobile-message user"><SessionMessageContent text={turn.user.text} className="mobile-markdown" /></section>
        <section className="mobile-message assistant">
          <header className="mobile-session-agent"><AgentIcon agentId={session.agent} /><strong>{{ codex: 'Codex', claude: 'Claude', traecli: 'TraeCode CLI', hermes: 'Hermes' }[session.agent]}</strong></header>
          <SessionTurnProcess key={`${session.id}:${turn.id}:${index === snapshot.data!.turns.length - 1}`} turn={turn} isLatest={index === snapshot.data!.turns.length - 1} />
          {turn.final && <SessionMessageContent text={turn.final.text} imageReferenceText={turn.user.text} className="mobile-markdown" />}
          <SessionTurnPending turn={turn} />
        </section>
      </article>)}
    </div>
  </section>;
}
