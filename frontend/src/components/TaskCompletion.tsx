import './task-completion.css';
import { CircleCheck } from 'lucide-react';

export function TaskCompletion() {
  const label = '任务已完成，点击查看';
  return <span className="task-completion" role="img" aria-label={label} title={label}>
    <CircleCheck size={14} aria-hidden="true" />
  </span>;
}
