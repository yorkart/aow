import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { ArrowDown, UserRound } from 'lucide-react';
import '../conversation-theme.css';
import './conversation-layout.css';

export type ConversationStatus = 'completed' | 'failed' | 'interrupted' | 'in_progress' | 'unknown';
export const conversationStatusNames: Record<ConversationStatus, string> = {
  completed: '已完成', failed: '失败', interrupted: '已中断', in_progress: '进行中', unknown: '执行结果未知',
};

export interface ConversationTurn {
  id: string;
  prompt: string;
  status?: ConversationStatus;
  user: ReactNode;
  userMeta?: ReactNode;
  assistant: ReactNode;
  assistantMeta?: ReactNode;
}

interface HeaderProps {
  title: string;
  agentName: string;
  agentIcon: ReactNode;
  workspace?: string;
  metadata?: ReactNode;
  actions?: ReactNode;
}

export function ConversationHeader({ title, agentIcon, workspace, metadata, actions }: HeaderProps) {
  return <header className="project-aow-snapshot-header">
    {agentIcon}
    <div className="project-aow-snapshot-heading">
      <h2 title={title}>{title}</h2>
      {workspace && <span className="project-aow-snapshot-workspace" title={workspace}>{workspace.split(/[/\\]/).filter(Boolean).at(-1) || workspace}</span>}
    </div>
    {metadata}
    {actions}
  </header>;
}

export function ConversationAgentMessage({ agentName, agentIcon, agentClassName = '', metadata, children }: {
  agentName: string; agentIcon: ReactNode; agentClassName?: string; metadata?: ReactNode; children: ReactNode;
}) {
  return <section className={`project-aow-snapshot-message assistant ${agentClassName}`}>
    <div className="project-aow-snapshot-bubble">
      <header className="project-aow-snapshot-message-header">
        {agentIcon}<strong>{agentName}</strong>
        {metadata && <span className="project-aow-snapshot-message-meta">{metadata}</span>}
      </header>
      {children}
    </div>
  </section>;
}

function promptPreview(text: string) {
  const normalized = text.replace(/\s+/g, ' ').trim();
  const characters = Array.from(normalized);
  return characters.length > 32 ? `${characters.slice(0, 32).join('')}…` : normalized || '空白输入';
}

// Presentation only: callers supply message contents and retain their process renderers.
export function ConversationLayout({ sessionId, turns, header, agentClassName, className = '', label, revision, banner, alternateContent, state, leadingContent, accessory, footer, footerExpanded = false, live = false, followRequest = 0, scrollClassName = '', scrollRef }: {
  sessionId: string;
  turns: ConversationTurn[];
  header: HeaderProps;
  agentClassName?: string;
  className?: string;
  label: string;
  revision?: unknown;
  banner?: ReactNode;
  alternateContent?: ReactNode;
  state?: ReactNode;
  leadingContent?: ReactNode;
  accessory?: ReactNode;
  footer?: ReactNode;
  footerExpanded?: boolean;
  live?: boolean;
  followRequest?: number;
  scrollClassName?: string;
  scrollRef?: RefObject<HTMLDivElement | null>;
}) {
  const localRef = useRef<HTMLDivElement>(null);
  const conversationRef = scrollRef ?? localRef;
  const footerRef = useRef<HTMLDivElement>(null);
  const chatRef = useRef<HTMLDivElement>(null);
  const turnRefs = useRef<Array<HTMLElement | null>>([]);
  const navigationRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [activeTurn, setActiveTurn] = useState(0);
  const [following, setFollowing] = useState(true);
  const followRef = useRef(true);
  const turnCount = turns.length;
  const hasMessages = turnCount > 0 || !!leadingContent;
  const hasFooter = !!footer;
  const setFollow = (value: boolean) => { followRef.current = value; setFollowing(value); };

  useLayoutEffect(() => {
    const conversation = conversationRef.current;
    const footer = footerRef.current;
    if (!conversation || !footer) return;
    // The fixed footer must use the same available width as the scrolling transcript.
    const alignFooter = () => { footer.style.paddingInlineEnd = `${conversation.offsetWidth - conversation.clientWidth}px`; };
    alignFooter();
    const observer = new ResizeObserver(alignFooter);
    observer.observe(conversation);
    return () => observer.disconnect();
  }, [conversationRef, hasFooter]);

  useEffect(() => {
    turnRefs.current.length = turnCount;
    navigationRefs.current.length = turnCount;
  }, [turnCount]);
  useEffect(() => {
    setActiveTurn(0);
    followRef.current = true; setFollowing(true);
    conversationRef.current?.scrollTo({ top: 0, behavior: 'instant' });
  }, [sessionId, conversationRef]);
  useEffect(() => {
    navigationRefs.current[activeTurn]?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
  }, [activeTurn]);

  const updateActiveTurn = useCallback(() => {
    const conversation = conversationRef.current;
    if (!conversation?.clientHeight || !turnRefs.current.length) return;
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
    setActiveTurn(current => current === next ? current : next);
  }, [conversationRef]);
  const scrollToEnd = useCallback(() => {
    const conversation = conversationRef.current;
    if (conversation?.clientHeight) conversation.scrollTo({ top: conversation.scrollHeight, behavior: 'instant' });
  }, [conversationRef]);
  useEffect(() => {
    if (live && followRef.current) scrollToEnd();
    updateActiveTurn();
  }, [sessionId, revision, live, scrollToEnd, updateActiveTurn]);
  useEffect(() => {
    if (!live) return;
    followRef.current = true; setFollowing(true); scrollToEnd();
  }, [followRequest, live, scrollToEnd]);
  useEffect(() => {
    if (!live || !chatRef.current) return;
    // Markdown, images and embedded editors may grow after the snapshot renders.
    const observer = new ResizeObserver(() => {
      if (followRef.current) scrollToEnd();
      updateActiveTurn();
    });
    observer.observe(chatRef.current);
    if (conversationRef.current) observer.observe(conversationRef.current);
    return () => observer.disconnect();
  }, [live, conversationRef, scrollToEnd, updateActiveTurn]);

  const navigateToTurn = (index: number) => {
    setFollow(false); setActiveTurn(index);
    const conversation = conversationRef.current;
    const turn = turnRefs.current[index];
    if (!conversation || !turn) return;
    conversation.scrollTo({
      top: conversation.scrollTop + turn.getBoundingClientRect().top - conversation.getBoundingClientRect().top - 18,
      behavior: 'instant',
    });
  };

  return <section className={`aow-conversation-layout aow-conversation-theme ${className}`} aria-label={label}>
    <ConversationHeader {...header} />
    {banner}
    {alternateContent}
    <div className={`project-aow-snapshot-body${turnCount ? '' : ' without-navigation'}`} hidden={!!alternateContent}>
      <div className="conversation-main">
        <div className={`project-aow-snapshot-conversation ${scrollClassName}`} hidden={!!footer && !hasMessages && !state} ref={conversationRef} onScroll={event => {
          if (!event.currentTarget.clientHeight) return;
          updateActiveTurn();
          if (live) { const element = event.currentTarget; setFollow(element.scrollHeight - element.scrollTop - element.clientHeight < 60); }
        }}>
          {state}
          <div className="project-aow-snapshot-chat" ref={chatRef}>
            {leadingContent}
            {turns.map((turn, index) => <article className="project-aow-snapshot-turn" key={turn.id} ref={node => { turnRefs.current[index] = node; }}>
              <section className="project-aow-snapshot-message user">
                <header className="project-aow-snapshot-message-header"><UserRound size={22} /><strong>User</strong>{turn.userMeta}</header>
                <div className="project-aow-snapshot-bubble">{turn.user}</div>
              </section>
              <ConversationAgentMessage agentName={header.agentName} agentIcon={header.agentIcon} agentClassName={agentClassName} metadata={turn.assistantMeta}>{turn.assistant}</ConversationAgentMessage>
            </article>)}
          </div>
        </div>
        {live && !following && hasMessages && <div className="conversation-follow"><button type="button" onClick={() => { setFollow(true); scrollToEnd(); }}><ArrowDown size={13} />跳到最新消息</button></div>}
        {accessory}
        {footer && <div ref={footerRef} className={`conversation-footer${!hasMessages ? ' empty' : ''}${footerExpanded ? ' expanded' : ''}`}>{footer}</div>}
      </div>
      {turnCount > 0 && <nav className="project-aow-snapshot-navigation" aria-label="用户输入导航">
        <header><strong>对话导航</strong><span>{turnCount}</span></header>
        <ol>{turns.map((turn, index) => <li key={turn.id}>
          <button className={activeTurn === index ? 'active' : undefined} type="button" title={turn.prompt} aria-current={activeTurn === index ? 'location' : undefined} ref={node => { navigationRefs.current[index] = node; }} onClick={() => navigateToTurn(index)}>
            <span>{index + 1}</span><strong>{promptPreview(turn.prompt)}</strong>
            {turn.status && <i className={turn.status} title={conversationStatusNames[turn.status]} />}
          </button>
        </li>)}</ol>
      </nav>}
    </div>
  </section>;
}
