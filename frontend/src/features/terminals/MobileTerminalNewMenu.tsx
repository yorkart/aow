import { useEffect, useId, useRef, useState } from 'react';
import { Plus, RefreshCw, TerminalSquare } from 'lucide-react';
import { AgentIcon } from '../agents/AgentIcon';
import { aowAgentType } from '../agents/agentTypes';
import { agentsApi } from '../agents/api';
import { useMobileResource } from '../../mobile/mobileState';

export function MobileTerminalNewMenu({ busy, onCreate }: { busy: boolean; onCreate: (agentId?: string) => void }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const agents = useMobileResource('terminal-new-agents', () => agentsApi.agents(), open);
  const available = agents.data?.filter(agent => agent.available) ?? [];
  useEffect(() => {
    if (!open) return;
    menu.current?.querySelector<HTMLButtonElement>('button')?.focus({ preventScroll: true });
    const outside = (event: PointerEvent) => {
      if (!container.current?.contains(event.target as Node)) setOpen(false);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape' || event.key === 'Tab') {
        if (event.key === 'Escape') event.preventDefault();
        setOpen(false);
        trigger.current?.focus({ preventScroll: true });
      }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const items = [...menu.current!.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
      if (!items.length) return;
      const current = items.indexOf(document.activeElement as HTMLButtonElement);
      const index = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
        : (current + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      items[index].focus({ preventScroll: true });
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('keydown', key);
    };
  }, [open]);
  const create = (agentId?: string) => {
    setOpen(false);
    trigger.current?.focus({ preventScroll: true });
    onCreate(agentId);
  };
  return <div ref={container} className="mobile-terminal-new">
    <button ref={trigger} className="mobile-icon-button" aria-label="新建终端" aria-haspopup="menu" aria-expanded={open}
      aria-controls={open ? id : undefined} disabled={busy} onClick={() => setOpen(value => !value)} onKeyDown={event => {
        if (event.key === 'ArrowDown' && !open) { event.preventDefault(); setOpen(true); }
      }}><Plus size={21} /></button>
    {open && <div ref={menu} id={id} className="mobile-terminal-new-menu" role="menu" aria-label="新建终端或 Agent">
      <button role="menuitem" disabled={busy} onClick={() => create()}><TerminalSquare size={21} /><span>Terminal</span></button>
      <div className="mobile-terminal-new-label">Agents</div>
      {available.map(agent => <button key={agent.id} role="menuitem" disabled={busy} onClick={() => create(agent.id)}>
        <AgentIcon agentId={aowAgentType(agent)} /><span>{agent.display_name}</span>
      </button>)}
      {agents.loading ? <p className="mobile-terminal-new-status" role="status">正在加载 Agent…</p>
        : agents.error ? <>
          <p className="mobile-terminal-new-status error" role="alert">Agent 加载失败：{agents.error}</p>
          <button role="menuitem" onClick={agents.reload}><RefreshCw size={18} /><span>重新加载 Agent</span></button>
        </> : !available.length ? <p className="mobile-terminal-new-status">未发现可用 Agent</p> : null}
    </div>}
  </div>;
}
