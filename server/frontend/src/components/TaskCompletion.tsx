import { t } from '../lib/i18n';
import './task-completion.css';
import { CircleCheck } from 'lucide-react';

export function TaskCompletion() {
  const label = t("components.task_completed_click_to_view");
  return <span className="task-completion" role="img" aria-label={label} title={label}>
    <CircleCheck size={14} aria-hidden="true" />
  </span>;
}
