import { t } from '../lib/i18n';
import { useEffect, useState } from 'react';
import { Check, Circle, Clock3, GitBranchPlus, LoaderCircle, Minus, TriangleAlert, X } from 'lucide-react';
import type { WorktreeCreationJob, WorktreeCreationStatus } from './types';
import { creationActive } from './useWorktreeCreations';
import './worktree-creation.css';

const labels: Record<WorktreeCreationStatus, string> = {
  pending: t("aow.waiting"), running: t("aow.running"), succeeded: t("aow.completed"), failed: t("aow.failed"), timed_out: t("aow.timed_out"), interrupted: t("aow.interrupted"), skipped: t("aow.skipped"),
};
const icons = { pending: Circle, running: LoaderCircle, succeeded: Check, failed: TriangleAlert, timed_out: Clock3, interrupted: TriangleAlert, skipped: Minus };
const seconds = (milliseconds: number) => t("aow.seconds", { count: Math.max(0, milliseconds / 1000).toFixed(1), value0: Math.max(0, milliseconds / 1000).toFixed(1) });

export function WorktreeCreationProgress({ job, error, onRefresh, onClose, onLogs, onRetry, onOpen }: {
  job?: WorktreeCreationJob;
  error: string;
  onRefresh: () => void;
  onClose: () => void;
  onLogs: () => void;
  onRetry: (job: WorktreeCreationJob) => void;
  onOpen: (job: WorktreeCreationJob) => void;
}) {
  const [now, setNow] = useState(Date.now);
  const active = !!job && creationActive(job);
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, [active]);
  return <div className="project-aow-modal-backdrop" onPointerDown={onClose}>
    <section className="project-aow-modal project-aow-dialog worktree-creation-progress" role="dialog" aria-modal="true" aria-labelledby="worktree-creation-progress-title" onPointerDown={event => event.stopPropagation()} onKeyDown={event => { if (event.key === 'Escape') onClose(); }}>
      <header><div><GitBranchPlus /><strong id="worktree-creation-progress-title">{t("aow.worktree_creation_progress")}</strong></div><button type="button" aria-label={t("aow.close")} onClick={onClose}><X /></button></header>
      <div className="project-aow-dialog-body">
        {job ? <>
          <p className="worktree-creation-target"><strong>{job.branch}</strong><code>{job.path}</code></p>
          <p role="status">{active ? t("aow.the_task_runs_in_the_background_close_this_dialog") : t("aow.creation_task", { value0: labels[job.status] })}</p>
          <ol className="worktree-creation-steps" aria-label={t("aow.creation_steps")}>
            {job.steps.map((step, index) => {
              const Icon = icons[step.status];
              const elapsed = step.duration_ms ?? (step.started_at ? now - Date.parse(step.started_at) : 0);
              return <li key={index} className={step.status}>
                <Icon size={16} className={step.status === 'running' ? 'spinning' : ''} />
                <div><strong>{index + 1}. {step.title}</strong><span>{labels[step.status]}{step.started_at ? t("aow.duration", { value0: seconds(elapsed) }) : ''} {t("aow.timeout")} {seconds(step.timeout_ms)}</span>
                  {step.message && <p>{step.message}</p>}
                  {step.status === 'running' && elapsed >= step.timeout_ms && <p>{t("aow.timeout_reached_waiting_for_confirmation_from_the_background_task")}</p>}
                </div>
              </li>;
            })}
          </ol>
          {job.error && <div className="project-aow-error" role="alert">{job.error}</div>}
        </> : <p>{t("aow.retrieving_task_status_if_the_task_finished_or_the")}</p>}
        {error && <div className="project-aow-error" role="alert">{error}<button type="button" onClick={onRefresh}>{t("aow.refresh_progress")}</button></div>}
      </div>
      <footer className="project-aow-dialog-footer">
        <button type="button" className="project-aow-dialog-button" onClick={onLogs}>{t("aow.view_logs")}</button>
        {job && !active && job.status !== 'succeeded' && <button type="button" className="project-aow-dialog-button" onClick={() => onRetry(job)}>{t("aow.edit_details")}</button>}
        {job?.status === 'succeeded' && <button type="button" className="project-aow-dialog-button primary" onClick={() => onOpen(job)}>{t("aow.open_worktree")}</button>}
        <button type="button" autoFocus className="project-aow-dialog-button" onClick={onClose}>{active ? t("aow.close_continue_in_background") : t("aow.close")}</button>
      </footer>
    </section>
  </div>;
}
