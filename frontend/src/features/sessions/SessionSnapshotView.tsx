import { SessionMessageContent } from './SessionMessageContent';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { AlertCircle, ArrowDownToLine, History, LoaderCircle, RefreshCw, UserRound } from 'lucide-react';
import type { AgentSessionSnapshot, AgentSessionTurnStatus, AowAgentSession } from './types';
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

const promptPreviewLength = 32;

const statusNames: Record<AgentSessionTurnStatus, string> = {
  completed: '已完成',
  failed: '失败',
  interrupted: '已中断',
  in_progress: '进行中',
};

function time(value: string | null) {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'medium' }).format(date);
}

function promptPreview(text: string) {
  const normalized = text.replace(/\s+/g, ' ').trim();
  if (!normalized) return '空白输入';
  const characters = Array.from(normalized);
  return characters.length > promptPreviewLength
    ? `${characters.slice(0, promptPreviewLength).join('')}…`
    : normalized;
}

export function SessionSnapshotView({ session, snapshot, loading, error, onRefresh, actions, publicView = false }: Props) {
  const conversationRef = useRef<HTMLDivElement>(null);
  const turnRefs = useRef<Array<HTMLElement | null>>([]);
  const navigationRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [activeTurn, setActiveTurn] = useState(0);
  const turnCount = snapshot?.turns.length ?? 0;
  const agentName = { codex: 'Codex', claude: 'Claude', traecli: 'TraeCode CLI' }[session.agent];

  useEffect(() => {
    turnRefs.current = turnRefs.current.slice(0, turnCount);
    navigationRefs.current = navigationRefs.current.slice(0, turnCount);
  }, [turnCount]);

  useEffect(() => {
    setActiveTurn(0);
    conversationRef.current?.scrollTo({ top: 0, behavior: 'instant' });
  }, [session.id]);

  useEffect(() => {
    navigationRefs.current[activeTurn]?.scrollIntoView({ block: 'nearest' });
  }, [activeTurn]);

  const updateActiveTurn = useCallback(() => {
    const conversation = conversationRef.current;
    if (!conversation || !turnRefs.current.length) return;
    if (conversation.scrollTop > 0 && conversation.scrollHeight - conversation.scrollTop - conversation.clientHeight <= 2) {
      setActiveTurn(turnRefs.current.length - 1);
      return;
    }
    const marker = conversation.getBoundingClientRect().top + Math.min(100, conversation.clientHeight * .25);
    let next = 0;
    for (let index = 0; index < turnRefs.current.length; index += 1) {
      const turn = turnRefs.current[index];
      if (!turn || turn.getBoundingClientRect().top > marker) break;
      next = index;
    }
    setActiveTurn((current) => current === next ? current : next);
  }, []);

  const navigateToTurn = (index: number) => {
    setActiveTurn(index);
    const conversation = conversationRef.current;
    const turn = turnRefs.current[index];
    if (!conversation || !turn) return;
    conversation.scrollTo({
      top: conversation.scrollTop + turn.getBoundingClientRect().top - conversation.getBoundingClientRect().top - 18,
      behavior: 'smooth',
    });
  };

  useEffect(() => { updateActiveTurn(); }, [snapshot, updateActiveTurn]);

  return <section className="project-aow-session-snapshot" aria-label={`${session.title} 会话快照`}>
    <header className="project-aow-snapshot-header">
      <AgentIcon agentId={session.agent} />
      <div className="project-aow-snapshot-heading">
        <h2 title={session.title}>{session.title}</h2>
        <div><span>{agentName}</span><span>·</span><span>{turnCount} 轮对话</span>{!publicView && <span className="project-aow-snapshot-workspace" title={`${session.cwd} · ${session.session_id}`}>{session.cwd}</span>}</div>
      </div>
      {snapshot ? <>
        <span className={`status ${snapshot.status}`}>{statusNames[snapshot.status]}</span>
        {snapshot.truncated ? <span className="warning">最近 200 轮</span> : null}
        <time className="project-aow-snapshot-captured" dateTime={snapshot.captured_at}>更新于 {time(snapshot.captured_at)}</time>
      </> : null}
      {turnCount > 1 && <button className="project-aow-snapshot-latest" type="button" onClick={() => navigateToTurn(turnCount - 1)}><ArrowDownToLine /><span>最新一轮</span></button>}
      {actions}
      <button title="刷新会话快照" aria-label="刷新会话快照" disabled={loading} onClick={onRefresh}><RefreshCw className={loading ? 'spinning' : ''} /></button>
    </header>
    <div className="project-aow-snapshot-body">
      <div className="project-aow-snapshot-conversation" ref={conversationRef} onScroll={updateActiveTurn}>
        {loading && !snapshot ? <div className="project-aow-snapshot-state"><LoaderCircle className="spinning" />正在读取会话快照…</div> : null}
        {error ? <div className="project-aow-snapshot-state error"><AlertCircle />{error}<button onClick={onRefresh}>重试</button></div> : null}
        {!loading && !error && snapshot && !snapshot.turns.length ? <div className="project-aow-snapshot-state"><History />这个会话还没有可展示的用户输入。</div> : null}
        {snapshot?.turns.length ? <div className="project-aow-snapshot-chat">
          {snapshot.turns.map((turn, index) => <article
            className="project-aow-snapshot-turn"
            key={turn.id}
            ref={(node) => { turnRefs.current[index] = node; }}
          >
            <div className="project-aow-snapshot-turn-label"><span>第 {index + 1} 轮</span>{index === turnCount - 1 && <span className="latest">最新一轮</span>}<span className={`turn-status ${turn.status}`}>{statusNames[turn.status]}</span></div>
            <section className="project-aow-snapshot-message user">
              <header className="project-aow-snapshot-message-header"><UserRound size={22} /><strong>User</strong>{turn.user.timestamp && <time dateTime={turn.user.timestamp}>{time(turn.user.timestamp)}</time>}</header>
              <div className="project-aow-snapshot-bubble">
                <SessionMessageContent text={turn.user.text} previewImages={!publicView} />
              </div>
            </section>
            <section className={`project-aow-snapshot-message assistant ${session.agent}`}>
              <div className="project-aow-snapshot-bubble">
                <header className="project-aow-snapshot-message-header"><AgentIcon agentId={session.agent} /><strong>{agentName}</strong>{turn.final?.timestamp && <time dateTime={turn.final.timestamp}>{time(turn.final.timestamp)}</time>}</header>
                <SessionTurnProcess key={`${session.id}:${turn.id}:${index === turnCount - 1}`} turn={turn} isLatest={index === turnCount - 1} />
                {turn.final && <div className="project-aow-snapshot-conclusion"><SessionMessageContent text={turn.final.text} imageReferenceText={turn.user.text} previewImages={!publicView} /></div>}
                <SessionTurnPending turn={turn} />
              </div>
            </section>
          </article>)}
        </div> : null}
      </div>
      {snapshot?.turns.length ? <nav className="project-aow-snapshot-navigation" aria-label="用户输入导航">
        <header><strong>对话导航</strong><span>{snapshot.turns.length}</span></header>
        <ol>
          {snapshot.turns.map((turn, index) => <li key={turn.id}>
            <button
              className={activeTurn === index ? 'active' : undefined}
              type="button"
              title={turn.user.text}
              aria-current={activeTurn === index ? 'location' : undefined}
              ref={(node) => { navigationRefs.current[index] = node; }}
              onClick={() => navigateToTurn(index)}
            >
              <span>{index + 1}</span>
              <strong>{promptPreview(turn.user.text)}</strong>
              <i className={turn.status} title={statusNames[turn.status]} />
            </button>
          </li>)}
        </ol>
      </nav> : null}
    </div>
  </section>;
}
