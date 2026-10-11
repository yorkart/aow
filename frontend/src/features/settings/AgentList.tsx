import { useRef, useState } from 'react';
import { Copy, GripVertical, Pencil, Trash2 } from 'lucide-react';
import { AgentIcon } from '../agents/AgentIcon';
import { agentTypes, aowAgentType } from '../agents/agentTypes';
import type { AowAgent } from '../agents/types';

interface Drag {
  id: string; x: number; y: number; moved: boolean; over?: string; after: boolean;
}

export function AgentList({ agents, busy, editingAgentId, onEdit, onCopy, onRemove, onReorder }: {
  agents: AowAgent[]; busy: boolean; editingAgentId?: string;
  onEdit: (agent: AowAgent) => void; onCopy: (agent: AowAgent) => void; onRemove: (id: string) => void;
  onReorder: (type: AowAgent['agent_type'], ids: string[]) => Promise<void>;
}) {
  const drag = useRef<Drag | undefined>(undefined);
  const saving = useRef(false);
  const [dragging, setDragging] = useState<Drag>();
  const [status, setStatus] = useState('');
  const [error, setError] = useState('');
  const cancelDrag = () => { drag.current = undefined; setDragging(undefined); };
  const move = async (type: AowAgent['agent_type'], items: AowAgent[], id: string, over: string, after: boolean) => {
    if (busy || saving.current || id === over) return;
    const ids = items.map(agent => agent.id).filter(item => item !== id);
    const index = ids.indexOf(over);
    if (index < 0) return;
    ids.splice(index + Number(after), 0, id);
    if (ids.every((item, position) => item === items[position].id)) return;
    saving.current = true;
    setError(''); setStatus('正在保存顺序…');
    try {
      await onReorder(type, ids);
      setStatus('顺序已保存');
    } catch (reason) {
      setStatus(''); setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      saving.current = false;
    }
  };
  return <div className="project-aow-agent-list">
    <p className="project-aow-form-intro">拖动手柄调整组内顺序，自动保存并同步到 Agent 菜单。Auto detected 固定在组首。</p>
    {[...agentTypes, { id: undefined, label: '未设置类型' }].map(type => {
      const group = agents.filter(agent => aowAgentType(agent) === type.id);
      if (!group.length) return null;
      const configured = group.filter(agent => agent.source === 'configured');
      const ordered = [...group.filter(agent => agent.source === 'detected'), ...configured];
      return <section className="project-aow-agent-group" aria-label={type.label} key={type.id ?? 'unknown'}>
        <h3>{type.label}</h3>
        {ordered.map(agent => <div className={`project-aow-agent-row${dragging?.id === agent.id ? ' dragging' : ''}${dragging?.over === agent.id ? dragging.after ? ' drop-after' : ' drop-before' : ''}`} data-agent-id={agent.id} key={agent.id}>
          {agent.source === 'configured' ? <button type="button" className="project-aow-agent-drag" aria-label={`调整 ${agent.display_name} 的顺序`} title="拖动排序，或使用上下方向键" aria-disabled={busy || configured.length < 2}
            onPointerDown={event => {
              if (busy || saving.current || configured.length < 2 || event.button !== 0 || !event.isPrimary) return;
              drag.current = { id: agent.id, x: event.clientX, y: event.clientY, moved: false, after: false };
              event.currentTarget.setPointerCapture(event.pointerId);
            }}
            onPointerMove={event => {
              const current = drag.current;
              if (!current) return;
              if (Math.hypot(current.x - event.clientX, current.y - event.clientY) > 5) current.moved = true;
              if (!current.moved) return;
              const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('[data-agent-id]');
              const valid = configured.some(item => item.id === target?.dataset.agentId && item.id !== current.id);
              current.over = valid ? target?.dataset.agentId : undefined;
              const bounds = target?.getBoundingClientRect();
              current.after = !!bounds && event.clientY > bounds.top + bounds.height / 2;
              setDragging({ ...current });
              const scroll = event.currentTarget.closest('.project-aow-dialog-body');
              const viewport = scroll?.getBoundingClientRect();
              if (scroll && viewport) {
                if (event.clientY < viewport.top + 36) scroll.scrollTop -= 18;
                else if (event.clientY > viewport.bottom - 36) scroll.scrollTop += 18;
              }
            }}
            onPointerUp={event => {
              const current = drag.current;
              cancelDrag();
              if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
              if (current?.moved && current.over) void move(type.id ?? null, configured, current.id, current.over, current.after);
            }}
            onPointerCancel={cancelDrag} onLostPointerCapture={cancelDrag}
            onKeyDown={event => {
              if (event.key === 'Escape' && drag.current) { event.stopPropagation(); cancelDrag(); return; }
              if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
              event.preventDefault();
              const next = configured[configured.indexOf(agent) + (event.key === 'ArrowUp' ? -1 : 1)];
              if (next) void move(type.id ?? null, configured, agent.id, next.id, event.key === 'ArrowDown');
            }}><GripVertical /></button> : <span className="project-aow-agent-drag-placeholder" />}
          <span className={`project-aow-agent-dot ${agent.available ? 'available' : ''}`} title={agent.available ? '可用' : '不可用，请检查类型和执行路径'} />
          <AgentIcon agentId={aowAgentType(agent)} />
          <div className="project-aow-agent-identity"><strong title={agent.display_name}>{agent.display_name}</strong><span title={agent.id}>{agent.id}</span></div>
          <small>{agent.source === 'detected' ? 'Auto detected' : 'Configured'}</small>
          <button type="button" className="project-aow-agent-edit" title={`编辑 ${agent.display_name}`} aria-pressed={editingAgentId === agent.id} disabled={busy} onClick={() => onEdit(agent)}><Pencil /></button>
          <button type="button" className="project-aow-agent-copy" title={`复制 ${agent.display_name}`} disabled={busy} onClick={() => onCopy(agent)}><Copy /></button>
          {agent.source === 'configured' && <button type="button" title="移除配置" disabled={busy} onClick={() => onRemove(agent.id)}><Trash2 /></button>}
        </div>)}
      </section>;
    })}
    {!agents.length && <p className="project-aow-empty">尚未发现本地 Agent，可在下方注册。</p>}
    {status && <p role="status">{status}</p>}
    {error && <div className="project-aow-error" role="alert">{error}</div>}
  </div>;
}
