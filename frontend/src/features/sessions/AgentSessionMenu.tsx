import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { AowAgent } from '../agents/types';
import type { AowAgentSession } from './types';
import { aowAgentType } from '../agents/agentTypes';
import { withFloatingOpen } from '../../aow/floatingWorkspaceState';
import { AgentIcon } from '../agents/AgentIcon';
import { WorkspaceMenuLabel } from '../../aow/WorkspaceMenuLabel';

export function AgentSessionMenu({ x, y, session, agents, onOpen, onResume, onClose }: {
  x: number;
  y: number;
  session: AowAgentSession;
  agents: AowAgent[];
  onOpen: (session: AowAgentSession) => void;
  onResume: (session: AowAgentSession, agent: AowAgent) => void;
  onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });
  const profiles = agents.filter(agent => agent.available && aowAgentType(agent) === session.agent);

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({
      x: Math.max(4, Math.min(x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - bounds.height - 4)),
    });
  }, [x, y, profiles.length]);

  useEffect(() => {
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    const scroll = (event: Event) => {
      if (!(event.target instanceof Node) || !menu.current?.contains(event.target)) onClose();
    };
    window.addEventListener('pointerdown', onClose);
    window.addEventListener('scroll', scroll, true);
    window.addEventListener('resize', onClose);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', onClose);
      window.removeEventListener('scroll', scroll, true);
      window.removeEventListener('resize', onClose);
      window.removeEventListener('keydown', key);
    };
  }, [onClose]);

  const action = (callback: () => void) => () => { onClose(); callback(); };
  return createPortal(<div ref={menu} className="project-aow-context-menu project-aow-session-menu"
    style={{ left: position.x, top: position.y }} role="menu" aria-label={`${session.title} 会话操作`}
    onPointerDown={event => event.stopPropagation()} onContextMenu={event => event.preventDefault()}>
    <button role="menuitem" onClick={action(() => onOpen(session))}><WorkspaceMenuLabel action="open" /></button>
    <button role="menuitem" onClick={action(() => withFloatingOpen(() => onOpen(session)))}><WorkspaceMenuLabel action="floating" /></button>
    {profiles.length ? <hr /> : null}
    {profiles.map(agent => <button key={agent.id} role="menuitem" title={`Resume · ${agent.display_name}`}
      onClick={action(() => onResume(session, agent))}>
      <AgentIcon agentId={session.agent} /><span>Resume · {agent.display_name}</span>
    </button>)}
  </div>, document.body);
}
