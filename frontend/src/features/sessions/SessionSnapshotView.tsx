import type { ReactNode } from 'react';
import { AlertCircle, History, LoaderCircle, RefreshCw } from 'lucide-react';
import { ConversationLayout, conversationStatusNames } from '../../components/conversation/ConversationLayout';
import { SessionMessageContent } from './SessionMessageContent';
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
  return new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'medium' }).format(date);
}

export function SessionSnapshotView({ session, snapshot, loading, error, onRefresh, actions, publicView = false }: Props) {
  const turnCount = snapshot?.turns.length ?? 0;
  const agentName = { codex: 'Codex', claude: 'Claude', traecli: 'TraeCode CLI', hermes: 'Hermes', pi: 'Pi' }[session.agent];
  return <ConversationLayout
    sessionId={session.id}
    revision={snapshot}
    className="project-aow-session-snapshot"
    label={`${session.title} 会话快照`}
    agentClassName={session.agent}
    header={{
      title: session.title, agentName, agentIcon: <AgentIcon agentId={session.agent} />,
      workspace: publicView ? undefined : session.cwd,
      metadata: snapshot && <>
        <span className={`status ${snapshot.status}`}>{conversationStatusNames[snapshot.status]}</span>
        {snapshot.truncated && <span className="warning">最近 200 轮</span>}
        <time className="project-aow-snapshot-captured" dateTime={snapshot.captured_at}>更新于 {time(snapshot.captured_at)}</time>
      </>,
      actions: <>{actions}<button title="刷新会话快照" aria-label="刷新会话快照" disabled={loading} onClick={onRefresh}><RefreshCw className={loading ? 'spinning' : ''} /></button></>,
    }}
    state={<>
      {loading && !snapshot && <div className="project-aow-snapshot-state"><LoaderCircle className="spinning" />正在读取会话快照…</div>}
      {error && <div className="project-aow-snapshot-state error"><AlertCircle />{error}<button onClick={onRefresh}>重试</button></div>}
      {!loading && !error && snapshot && !snapshot.turns.length && <div className="project-aow-snapshot-state"><History />这个会话还没有可展示的用户输入。</div>}
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
  />;
}
