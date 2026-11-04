import { t } from '../../lib/i18n';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Pause, Pencil, Trash2, Zap } from 'lucide-react';
import { WorkspaceMenuLabel } from '../../aow/WorkspaceMenuLabel';
import type { AutomationTask } from './types';

export function AutomationTaskMenu({ x, y, task, busy, onClose, onOpen, onOpenFloating, onEdit, onToggle, onDelete }: {
  x: number;
  y: number;
  task: AutomationTask;
  busy: boolean;
  onClose: () => void;
  onOpen: () => void;
  onOpenFloating: () => void;
  onEdit: () => void;
  onToggle: () => void;
  onDelete: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({
      x: Math.max(4, Math.min(x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - bounds.height - 4)),
    });
  }, [x, y]);

  useEffect(() => {
    const close = () => onClose();
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('pointerdown', close);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('keydown', key);
    };
  }, [onClose]);

  const action = (callback: () => void) => () => { onClose(); callback(); };
  return createPortal(<div ref={menu} className="project-aow-context-menu automation-task-menu"
    style={{ left: position.x, top: position.y }} role="menu" aria-label={t("automations.automation_actions", { value0: task.name })}
    onPointerDown={event => event.stopPropagation()} onContextMenu={event => event.preventDefault()}>
    <button role="menuitem" onClick={action(onOpen)}><WorkspaceMenuLabel action="open" /></button>
    <button role="menuitem" onClick={action(onOpenFloating)}><WorkspaceMenuLabel action="floating" /></button>
    <hr />
    <button role="menuitem" disabled={busy} onClick={action(onEdit)}><Pencil />{t("acp.edit")}</button>
    {task.kind !== 'manual' ? <button role="menuitem" disabled={busy} onClick={action(onToggle)}>{task.enabled ? <Pause /> : <Zap />}{task.enabled ? t("automations.pause") : t("automations.enable")}</button> : null}
    <button role="menuitem" className="danger" disabled={busy} onClick={action(onDelete)}><Trash2 />{t("acp.delete")}</button>
  </div>, document.body);
}
