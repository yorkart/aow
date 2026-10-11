import { useEffect, useState } from 'react';
import { Check, Circle, Clock3, GitBranchPlus, LoaderCircle, Minus, TriangleAlert, X } from 'lucide-react';
import type { WorktreeCreationJob, WorktreeCreationStatus } from './types';
import { creationActive } from './useWorktreeCreations';
import './worktree-creation.css';

const labels: Record<WorktreeCreationStatus, string> = {
  pending: '等待中', running: '执行中', succeeded: '已完成', failed: '失败', timed_out: '超时', interrupted: '已中断', skipped: '已跳过',
};
const icons = { pending: Circle, running: LoaderCircle, succeeded: Check, failed: TriangleAlert, timed_out: Clock3, interrupted: TriangleAlert, skipped: Minus };
const seconds = (milliseconds: number) => `${Math.max(0, milliseconds / 1000).toFixed(1)} 秒`;

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
      <header><div><GitBranchPlus /><strong id="worktree-creation-progress-title">创建 Worktree 进度</strong></div><button type="button" aria-label="关闭" onClick={onClose}><X /></button></header>
      <div className="project-aow-dialog-body">
        {job ? <>
          <p className="worktree-creation-target"><strong>{job.branch}</strong><code>{job.path}</code></p>
          <p role="status">{active ? '任务在后台执行，可关闭弹框，从底部状态栏查看进度。' : `创建任务${labels[job.status]}`}</p>
          <ol className="worktree-creation-steps" aria-label="创建步骤">
            {job.steps.map((step, index) => {
              const Icon = icons[step.status];
              const elapsed = step.duration_ms ?? (step.started_at ? now - Date.parse(step.started_at) : 0);
              return <li key={index} className={step.status}>
                <Icon size={16} className={step.status === 'running' ? 'spinning' : ''} />
                <div><strong>{index + 1}. {step.title}</strong><span>{labels[step.status]}{step.started_at ? ` · 耗时 ${seconds(elapsed)}` : ''} · 超时上限 {seconds(step.timeout_ms)}</span>
                  {step.message && <p>{step.message}</p>}
                  {step.status === 'running' && elapsed >= step.timeout_ms && <p>已达到超时上限，正在等待后台确认结果…</p>}
                </div>
              </li>;
            })}
          </ol>
          {job.error && <div className="project-aow-error" role="alert">{job.error}</div>}
        </> : <p>正在获取任务状态；若任务已结束或服务已重启，可查看操作日志确认结果。</p>}
        {error && <div className="project-aow-error" role="alert">{error}<button type="button" onClick={onRefresh}>刷新进度</button></div>}
      </div>
      <footer className="project-aow-dialog-footer">
        <button type="button" className="project-aow-dialog-button" onClick={onLogs}>查看日志</button>
        {job && !active && job.status !== 'succeeded' && <button type="button" className="project-aow-dialog-button" onClick={() => onRetry(job)}>重新填写</button>}
        {job?.status === 'succeeded' && <button type="button" className="project-aow-dialog-button primary" onClick={() => onOpen(job)}>打开 Worktree</button>}
        <button type="button" autoFocus className="project-aow-dialog-button" onClick={onClose}>{active ? '关闭（后台继续）' : '关闭'}</button>
      </footer>
    </section>
  </div>;
}
