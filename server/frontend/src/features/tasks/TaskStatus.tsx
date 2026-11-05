import { useId, useRef, useState } from 'react';
import { Ban, Circle, CircleCheck, CircleDashed, CircleDot, CircleMinus, createLucideIcon } from 'lucide-react';
import { RowMenu } from '../../components/RowMenu';
import { t } from '../../lib/i18n';
import { taskStatuses, type TaskStatus } from './api';
import './tasks.css';

const Progress = createLucideIcon('TaskProgress', [
  ['circle', { cx: '12', cy: '12', r: '10', pathLength: '100', strokeDasharray: '80 20', key: 'progress' }],
]);
const icons = { backlog: CircleDashed, todo: Circle, in_progress: Progress, in_review: CircleDot, done: CircleCheck, cancelled: Ban, blocked: CircleMinus };

export function TaskStatusIcon({ status, size = 16 }: { status: TaskStatus; size?: number }) {
  const Icon = icons[status];
  return <Icon size={size} aria-hidden="true" className={`task-status-icon task-status-${status}`} />;
}

export function TaskStatusPicker({ status, disabled, onChange }: { status: TaskStatus; disabled: boolean; onChange: (status: TaskStatus) => void }) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  return <>
    <button ref={trigger} type="button" className="task-status-picker" aria-disabled={disabled} aria-label={t('tasks.business_status')} title={t('tasks.status_' + status)}
      aria-describedby={`${id}-value`} aria-haspopup="listbox" aria-expanded={open} aria-controls={open ? id : undefined}
      onClick={() => { if (!disabled) setOpen(value => !value); }} onKeyDown={event => {
        if (!disabled && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) { event.preventDefault(); setOpen(true); }
      }}>
      <TaskStatusIcon status={status} /><span id={`${id}-value`}>{t('tasks.status_' + status)}</span>
    </button>
    {open && !disabled && <RowMenu id={id} label={t('tasks.business_status')} role="listbox" className="task-status-options" anchor={trigger} onClose={() => setOpen(false)}>
      {taskStatuses.map(value => <button key={value} type="button" role="option" aria-selected={value === status} tabIndex={-1} onClick={() => {
        setOpen(false); trigger.current?.focus(); if (value !== status) onChange(value);
      }}><TaskStatusIcon status={value} /><span>{t('tasks.status_' + value)}</span></button>)}
    </RowMenu>}
  </>;
}
