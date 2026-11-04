import { t, getLanguage } from '../../lib/i18n';
import type { ReactNode } from 'react';
import { AlertCircle, History, LoaderCircle, RefreshCw } from 'lucide-react';
import { ConversationLayout, conversationStatusNames } from '../../components/conversation/ConversationLayout';
import { ConversationIds } from '../../components/conversation/ConversationIds';
import { ConversationAction } from '../../components/conversation/ConversationAction';
import { SessionMessageContent } from './SessionMessageContent';
import { WorkspaceMarkdownProvider } from '../files/WorkspaceMarkdown';
import { TokenUsageBadge } from '../../components/TokenUsageBadge';
import type { AgentSessionSnapshot, AowAgentSession } from './types';
import { AgentIcon } from '../agents/AgentIcon';
import { SessionTurnPending, SessionTurnProcess } from './SessionTurnProcess';

interface Props {
  session: Pick<AowAgentSession, 'id' | 'agent' | 'title'> & Partial<Pick<AowAgentSession, 'cwd' | 'session_id'>>;
  snapshot?: AgentSessionSnapshot;
  loading: boolean;
  error?: string;
  onRefresh: () => void;
  actions?: ReactNode;
  publicView?: boolean;
}

function time(value: string | null) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(getLanguage(), { dateStyle: 'medium', timeStyle: 'medium' }).format(date);
}

export function SessionSnapshotView({ session, snapshot, loading, error, onRefresh, actions, publicView = false }: Props) {
  const turnCount = snapshot?.turns.length ?? 0;
  const agentName = { codex: 'Codex', claude: 'Claude', traecli: 'TraeCode CLI', hermes: 'Hermes', pi: 'Pi' }[session.agent];
  return <WorkspaceMarkdownProvider directory={session.cwd} enabled={!publicView}><ConversationLayout
    sessionId={session.id}
    revision={snapshot}
    className="project-aow-session-snapshot"
    label={t("sessions.session_snapshot", { value0: session.title })}
    agentClassName={session.agent}
    header={{
      title: session.title, agentName, agentIcon: <AgentIcon agentId={session.agent} />,
      workspace: publicView ? undefined : session.cwd,
      metadata: snapshot && <>
        <span className={`status ${snapshot.status}`}>{conversationStatusNames[snapshot.status]}</span>
        {snapshot.truncated && <span className="warning">{t("sessions.latest_200_turns")}</span>}
        <time className="project-aow-snapshot-captured" dateTime={snapshot.captured_at}>{t("pr.updated")} {time(snapshot.captured_at)}</time>
      </>,
      actions: <>{actions}{!publicView && <ConversationIds identifiers={[{ label: t("navigation.session_id"), value: session.session_id ?? snapshot?.session_id }]} />}<ConversationAction label={t("sessions.refresh_session_snapshot")} disabled={loading} onClick={onRefresh}><RefreshCw className={loading ? 'spinning' : ''} /></ConversationAction></>,
    }}
    state={<>
      {loading && !snapshot && <div className="project-aow-snapshot-state"><LoaderCircle className="spinning" />{t("sessions.reading_session_snapshot")}</div>}
      {error && <div className="project-aow-snapshot-state error"><AlertCircle />{error}<button onClick={onRefresh}>{t("aow.retry")}</button></div>}
      {!loading && !error && snapshot && !snapshot.turns.length && <div className="project-aow-snapshot-state"><History />{t("sessions.this_session_has_no_user_input_to_display_yet")}</div>}
    </>}
    turns={(snapshot?.turns ?? []).map((turn, index) => ({
      id: turn.id, prompt: turn.user.text, status: turn.status,
      user: <SessionMessageContent text={turn.user.text} className="project-aow-snapshot-markdown conversation-prose" previewImages={!publicView} />,
      userMeta: turn.user.timestamp && <time dateTime={turn.user.timestamp}>{time(turn.user.timestamp)}</time>,
      assistantMeta: <><TokenUsageBadge usage={turn.usage} split />{turn.final?.timestamp && <time dateTime={turn.final.timestamp}>{time(turn.final.timestamp)}</time>}</>,
      assistant: <>
        <SessionTurnProcess key={`${session.id}:${turn.id}:${index === turnCount - 1}`} turn={turn} isLatest={index === turnCount - 1} />
        {turn.final && <div className="project-aow-snapshot-conclusion"><SessionMessageContent text={turn.final.text} className="project-aow-snapshot-markdown conversation-prose" imageReferenceText={turn.user.text} previewImages={!publicView} /></div>}
        <SessionTurnPending turn={turn} />
      </>,
    }))}
  /></WorkspaceMarkdownProvider>;
}
