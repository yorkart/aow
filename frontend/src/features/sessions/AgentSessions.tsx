import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { History, LoaderCircle, RefreshCw } from 'lucide-react';
import type { AowAgent } from '../agents/types';
import type { AowAgentSession } from './types';
import { sessionsApi } from './api';
import { AgentIcon } from '../agents/AgentIcon';
import { aowAgentType } from '../agents/agentTypes';
import { AowIconButton } from '../../components/AowIconButton';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { AowListRow } from '../../components/AowListRow';
import { AgentSessionMenu } from './AgentSessionMenu';

type SessionAgent = AowAgentSession['agent'];

interface Props {
  worktreePath: string;
  agents: AowAgent[];
  activeSessionId?: string;
  visible: boolean;
  onOpen: (session: AowAgentSession) => void;
  onResume: (session: AowAgentSession, agent: AowAgent) => void;
}

interface AgentSection {
  id: SessionAgent;
  title: string;
}

interface AgentSessionState {
  sessions: AowAgentSession[];
  loading: boolean;
  loaded: boolean;
  error: string;
}

const emptyState: AgentSessionState = { sessions: [], loading: false, loaded: false, error: '' };

function sessionAgent(id: string): AgentSection | undefined {
  switch (id.toLowerCase()) {
    case 'claude':
    case 'claude-code':
      return { id: 'claude', title: 'Claude Code' };
    case 'codex':
      return { id: 'codex', title: 'Codex' };
    case 'traecli':
      return { id: 'traecli', title: 'TraeCode CLI' };
    case 'hermes':
      return { id: 'hermes', title: 'Hermes' };
    default:
      return undefined;
  }
}

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

function relativeTime(value: string) {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return value;
  const seconds = Math.round((timestamp - Date.now()) / 1000);
  const ranges: [Intl.RelativeTimeFormatUnit, number][] = [
    ['year', 31_536_000], ['month', 2_592_000], ['week', 604_800],
    ['day', 86_400], ['hour', 3_600], ['minute', 60],
  ];
  const formatter = new Intl.RelativeTimeFormat('zh-CN', { numeric: 'auto' });
  for (const [unit, size] of ranges) {
    if (Math.abs(seconds) >= size) return formatter.format(Math.round(seconds / size), unit);
  }
  return formatter.format(seconds, 'second');
}

function absoluteTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat('zh-CN', {
    dateStyle: 'medium',
    timeStyle: 'medium',
  }).format(date);
}

export function AgentSessions({ worktreePath, agents, activeSessionId, visible, onOpen, onResume }: Props) {
  const sections = useMemo(() => {
    const found = new Map<SessionAgent, AgentSection>();
    for (const agent of agents) {
      const section = sessionAgent(aowAgentType(agent) ?? '');
      if (section && !found.has(section.id)) found.set(section.id, section);
    }
    return [...found.values()];
  }, [agents]);
  const [states, setStates] = useState<Partial<Record<SessionAgent, AgentSessionState>>>({});
  const [collapsed, setCollapsed] = useState<Partial<Record<SessionAgent, boolean>>>({});
  const [menu, setMenu] = useState<{ session: AowAgentSession; x: number; y: number }>();
  const closeMenu = useCallback(() => setMenu(undefined), []);
  const loading = useRef(new Set<SessionAgent>());
  const loadedOnOpen = useRef(false);

  useEffect(() => {
    setStates({});
    closeMenu();
    loadedOnOpen.current = false;
    return () => { loading.current = new Set(); };
  }, [worktreePath, closeMenu]);

  const load = useCallback(async (agent: SessionAgent) => {
    const pending = loading.current;
    if (pending.has(agent)) return;
    pending.add(agent);
    setStates((current) => ({
      ...current,
      [agent]: { ...(current[agent] ?? emptyState), loading: true, error: '' },
    }));
    try {
      const sessions = await sessionsApi.agentSessions(worktreePath, agent);
      if (loading.current !== pending) return;
      setStates((current) => ({ ...current, [agent]: { sessions, loading: false, loaded: true, error: '' } }));
    } catch (reason) {
      if (loading.current !== pending) return;
      setStates((current) => ({
        ...current,
        [agent]: { ...(current[agent] ?? emptyState), loading: false, loaded: true, error: errorMessage(reason) },
      }));
    } finally {
      pending.delete(agent);
    }
  }, [worktreePath]);

  useEffect(() => {
    if (!visible) closeMenu();
  }, [visible, closeMenu]);

  useEffect(() => {
    if (!visible || !sections.length || loadedOnOpen.current) return;
    loadedOnOpen.current = true;
    for (const section of sections) void load(section.id);
  }, [load, sections, visible]);

  return <AowPanelStack className="project-aow-sessions" role="region" aria-label="Conversation">
    {sections.map((section) => {
      const state = states[section.id] ?? emptyState;
      const isCollapsed = collapsed[section.id] ?? (state.loaded && !state.error && state.sessions.length === 0);
      const sectionId = `agent-sessions-${section.id}`;
      return <AowPanel className="project-aow-agent-sessions" key={section.id} bodyClassName="project-aow-agent-session-list" bodyRole="list"
          title={section.title} icon={<AgentIcon agentId={section.id} />} collapsed={isCollapsed}
          controlsId={sectionId} onCollapsedChange={next => setCollapsed(current => ({ ...current, [section.id]: next }))}
          actions={<AowIconButton title={`刷新 ${section.title} Sessions`} aria-label={`刷新 ${section.title} Sessions`} disabled={state.loading} onClick={() => void load(section.id)}>
            <RefreshCw className={state.loading ? 'spinning' : ''} />
          </AowIconButton>}
        >
          {!state.loaded && !state.loading ? <div className="project-aow-session-state"><History />等待打开 Conversation 加载</div> : null}
          {state.loading && !state.sessions.length ? <div className="project-aow-session-state"><LoaderCircle className="spinning" />正在扫描本地会话…</div> : null}
          {state.error ? <div className="project-aow-session-state error">{state.error}</div> : null}
          {state.loaded && !state.loading && !state.error && !state.sessions.length ? <div className="project-aow-session-state">暂无 Session</div> : null}
          {state.sessions.map((session) => <AowListRow className={`project-aow-session-row${activeSessionId === session.id ? ' active' : ''}`}
            icon={<AgentIcon agentId={session.agent} />}
            role="listitem" key={session.id} title={session.title} tooltip={session.cwd} menuLabel="会话操作" onOpen={() => onOpen(session)}
            menuExpanded={menu?.session.id === session.id} onMenu={(x, y) => setMenu({ session, x, y })}>
            <span className="project-aow-session-meta"><code>{session.session_id}</code><time dateTime={session.updated_at} title={absoluteTime(session.updated_at)}>{relativeTime(session.updated_at)}</time></span>
          </AowListRow>)}
      </AowPanel>;
    })}
    {!sections.length ? <div className="project-aow-session-empty"><History /><strong>暂无可用的 Session Agent</strong><span>请先注册 Claude Code、Codex、TraeCode CLI 或 Hermes。</span></div> : null}
    {visible && menu ? <AgentSessionMenu {...menu} agents={agents} onOpen={onOpen} onResume={onResume} onClose={closeMenu} /> : null}
  </AowPanelStack>;
}
