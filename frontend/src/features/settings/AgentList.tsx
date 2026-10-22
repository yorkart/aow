import { t } from '../../lib/i18n';
import { useRef, useState } from 'react';
import { Copy, GripVertical, Pencil, Trash2 } from 'lucide-react';
import { AgentIcon } from '../agents/AgentIcon';
import { agentTypes, aowAgentType } from '../agents/agentTypes';
import type { AowAgent } from '../agents/types';

interface Drag {
  id: string; x: number; y: number; moved: boolean; over?: string; after: boolean;
}

export function AgentList({ agents, busy, editingAgentId, onEdit, onCopy, onRemove, onReorder, onDefaultChange }: {
  agents: AowAgent[]; busy: boolean; editingAgentId?: string;
  onEdit: (agent: AowAgent) => void; onCopy: (agent: AowAgent) => void; onRemove: (id: string) => void;
  onReorder: (type: AowAgent['agent_type'], ids: string[]) => Promise<void>;
  onDefaultChange: (id: string) => void;
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
    setError(''); setStatus(t("settings.saving_order"));
    try {
      await onReorder(type, ids);
      setStatus(t("settings.order_saved"));
    } catch (reason) {
      setStatus(''); setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      saving.current = false;
    }
  };
  return <div className="project-aow-agent-list">
    <p className="project-aow-form-intro">{t("settings.drag_handles_to_reorder_within_groups_changes_autosave_and")}</p>
    {[...agentTypes, { id: undefined, label: t("settings.type_not_set") }].map(type => {
      const group = agents.filter(agent => aowAgentType(agent) === type.id);
      if (!group.length) return null;
      const configured = group.filter(agent => agent.source === 'configured');
      const ordered = [...group.filter(agent => agent.source === 'detected'), ...configured];
      return <section className="project-aow-agent-group" aria-label={type.label} key={type.id ?? 'unknown'}>
        <h3>{type.label}</h3>
        {ordered.map(agent => <div className={`project-aow-agent-row${dragging?.id === agent.id ? ' dragging' : ''}${dragging?.over === agent.id ? dragging.after ? ' drop-after' : ' drop-before' : ''}`} data-agent-id={agent.id} key={agent.id}>
          {agent.source === 'configured' ? <button type="button" className="project-aow-agent-drag" aria-label={t("settings.reorder", { value0: agent.display_name })} title={t("inbox.drag_to_reorder_or_use_the_up_and_down")} aria-disabled={busy || configured.length < 2}
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
          <span className={`project-aow-agent-dot ${agent.available ? 'available' : ''}`} title={agent.available ? t("settings.available") : t("settings.unavailable_check_the_type_and_executable_path")} />
          <AgentIcon agentId={aowAgentType(agent)} />
          <div className="project-aow-agent-identity"><strong title={agent.display_name}>{agent.display_name}</strong><span title={agent.id}>{agent.id}</span></div>
          <div className="project-aow-agent-options">
            <small>{agent.source === 'detected' ? t("common.auto_detected") : t("im.configured")}</small>
            <label className="project-aow-agent-default" title={agent.is_default ? t("acp.clear_default_and_use_the_first_item") : t("acp.set_as_default_instance")}>
              <input type="checkbox" aria-label={t("settings.default_agent", { value0: agent.display_name })} checked={!!agent.is_default} disabled={busy || !aowAgentType(agent)} onChange={event => onDefaultChange(event.target.checked ? agent.id : '')} />{t("aow.default")}</label>
          </div>
          <button type="button" className="project-aow-agent-edit" title={t("settings.edit", { value0: agent.display_name })} aria-pressed={editingAgentId === agent.id} disabled={busy} onClick={() => onEdit(agent)}><Pencil /></button>
          <button type="button" className="project-aow-agent-copy" title={t("components.copy", { value0: agent.display_name })} disabled={busy} onClick={() => onCopy(agent)}><Copy /></button>
          {agent.source === 'configured' ? <button type="button" title={t("settings.remove_configuration")} disabled={busy} onClick={() => onRemove(agent.id)}><Trash2 /></button> : <span className="project-aow-agent-action-placeholder" aria-hidden="true" />}
        </div>)}
      </section>;
    })}
    {!agents.length && <p className="project-aow-empty">{t("settings.no_local_agents_found_click_add_agent_to_register")}</p>}
    {status && <p role="status">{status}</p>}
    {error && <div className="project-aow-error" role="alert">{error}</div>}
  </div>;
}
