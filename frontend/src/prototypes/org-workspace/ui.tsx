import { useEffect, useId, useRef, type ReactNode } from 'react';
import { Check, Circle, LoaderCircle, Pause, X } from 'lucide-react';
import { statusLabels, type Agent, type TaskStatus } from './model';

export function Status({ status, label = true }: { status: TaskStatus; label?: boolean }) {
  const Icon = status === 'running' ? LoaderCircle : status === 'done' ? Check : status === 'paused' ? Pause : Circle;
  return <span className={`ow-status ow-status-${status}`}><Icon aria-hidden="true" />{label && statusLabels[status]}</span>;
}

export function Avatar({ agent, small = false }: { agent?: Agent; small?: boolean }) {
  return <span className={`ow-avatar ow-${agent?.color ?? 'mint'} ${small ? 'ow-avatar-small' : ''}`} aria-hidden="true">{agent?.name.slice(0, 1) ?? 'A'}</span>;
}

export function Modal({ title, children, onClose }: { title: string; children: ReactNode; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const heading = useId();
  useEffect(() => {
    const dialog = ref.current!;
    dialog.showModal();
    return () => dialog.close();
  }, []);
  return <dialog ref={ref} className="ow-dialog" aria-labelledby={heading} onCancel={onClose}>
    <header><h2 id={heading}>{title}</h2><button type="button" className="ow-icon-button" aria-label="关闭对话框" onClick={onClose}><X /></button></header>
    {children}
  </dialog>;
}

export function EmptyState({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return <div className="ow-empty">{icon}<h3>{title}</h3>{children}</div>;
}
