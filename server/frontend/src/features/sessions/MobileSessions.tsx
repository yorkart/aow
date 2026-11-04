import { t } from '../../lib/i18n';
import { useState } from 'react';
import { ChevronRight, MessageSquare, RefreshCw, Share2 } from 'lucide-react';
import { MobileConversationHeader } from '../../mobile/MobileConversationHeader';
import { TokenUsageBadge } from '../../components/TokenUsageBadge';
import { sessionsApi } from './api';
import { AgentIcon } from '../agents/AgentIcon';
import { SessionShareDialog } from './SessionShareButton';
import { SessionMessageContent } from './SessionMessageContent';
import { WorkspaceMarkdownProvider } from '../files/WorkspaceMarkdown';
import { SessionTurnPending, SessionTurnProcess } from './SessionTurnProcess';
import type { AowAgentSession } from './types';
import { MobilePageHeader, MobileRefresh, MobileState, mobileTime } from '../../mobile/MobilePrimitives';
import { useMobileResource, useMobileScroll, type MobileRoute, type MobileNavigate } from '../../mobile/mobileState';

const agents = ['codex', 'claude', 'traecli', 'hermes', 'pi'] as const;
const statusNames = { completed: t("aow.completed"), failed: t("aow.failed"), interrupted: t("aow.interrupted"), in_progress: t("components.in_progress") };

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
    {route.session ? selected ? <MobileSessionReader key={selected.id} session={selected} workspace={route.cwd ?? selected.cwd} back={back} />
      : <><MobilePageHeader title={t("sessions.session")} back={back} /><MobileState loading={list.loading} error={list.error} retry={list.reload} empty={t("sessions.session_not_found_return_to_the_list_and_refresh")} /></>
      : <>
        <MobilePageHeader title={t("navigation.conversation")} subtitle={t("acp.local_session_records")} actions={<MobileRefresh reload={list.reload} loading={list.loading} />} />
        <div className="mobile-scroll" ref={scroll}>
          <MobileState error={list.error} loading={list.loading && !list.data} retry={list.reload} empty={list.data?.sessions.length === 0 ? t("sessions.this_workspace_has_no_session_records_yet") : undefined} />
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
  const [sharing, setSharing] = useState(false);
  const snapshot = useMobileResource(`snapshot.${workspace}.${session.id}`, () => sessionsApi.agentSessionSnapshot(session, workspace));
  const scroll = useMobileScroll(`snapshot.${workspace}.${session.id}`, !!snapshot.data);
  return <WorkspaceMarkdownProvider directory={workspace}><section className="mobile-content-page">
    <MobileConversationHeader title={session.title} subtitle={snapshot.data ? `${session.agent} · ${statusNames[snapshot.data.status]}` : session.agent}
      back={back} identifiers={[
        { label: t("navigation.session_id"), value: session.session_id },
        { label: t("automations.workspace"), value: workspace },
      ]} actions={[
        { label: t("sessions.share"), icon: <Share2 size={18} />, disabled: !snapshot.data, onClick: () => setSharing(true) },
        { label: t("files.refresh"), icon: <RefreshCw size={18} />, disabled: snapshot.loading, onClick: snapshot.reload },
      ]} />
    {sharing && <SessionShareDialog session={session} workspacePath={workspace} close={() => setSharing(false)} />}
    <div className="mobile-scroll mobile-conversation" ref={scroll}>
      <MobileState error={snapshot.error} loading={snapshot.loading && !snapshot.data} retry={snapshot.reload} empty={snapshot.data?.turns.length === 0 ? t("sessions.this_session_has_no_conversation_to_display_yet") : undefined} />
      {snapshot.data && <p className="mobile-reader-meta"><MessageSquare size={13} />{t("sessions.snapshot_updated")} {mobileTime(snapshot.data.captured_at)}{snapshot.data.truncated && t("sessions.showing_the_latest_200_turns_only")}</p>}
      {snapshot.data?.turns.map((turn, index) => <article className="mobile-turn" key={turn.id}>
        <section className="mobile-message user"><SessionMessageContent text={turn.user.text} className="mobile-markdown" /></section>
        <section className="mobile-message assistant">
          <header className="mobile-session-agent">
            <AgentIcon agentId={session.agent} /><strong>{{ codex: 'Codex', claude: 'Claude', traecli: 'TraeCode CLI', hermes: 'Hermes', pi: 'Pi' }[session.agent]}</strong>
            <span className="mobile-session-agent-meta"><TokenUsageBadge usage={turn.usage} split />{turn.final?.timestamp && <time dateTime={turn.final.timestamp}>{mobileTime(turn.final.timestamp)}</time>}</span>
          </header>
          <SessionTurnProcess key={`${session.id}:${turn.id}:${index === snapshot.data!.turns.length - 1}`} turn={turn} isLatest={index === snapshot.data!.turns.length - 1} />
          {turn.final && <SessionMessageContent text={turn.final.text} imageReferenceText={turn.user.text} className="mobile-markdown" />}
          <SessionTurnPending turn={turn} />
        </section>
      </article>)}
    </div>
  </section></WorkspaceMarkdownProvider>;
}
