import { useEffect, useState } from 'react';
import { AlertTriangle, CircleCheck } from 'lucide-react';
import { aowApi } from '../../aow/aowApi';
import { useAowTabNavigation } from '../../aow/AowTabEntry';
import { tabTargetUrl, type TabTarget } from '../../aow/tabRoutes';
import type { TerminalHosting } from './types';
import './terminal-hosting.css';

const hostingPhases: Record<TerminalHosting['phase'], string> = {
  waiting: '等待本轮完成', reviewing: '检查中', collecting: '读取结论', submitting: '提交反馈', failed: '已暂停',
  completed: '审查通过', limit_reached: '已达输入上限',
};

export function hostingStatus(hosting: TerminalHosting) {
  const state = hosting.phase === 'completed' ? '托管已结束' : hosting.phase === 'limit_reached' ? '托管已停止' : '托管中';
  return `${state} · ${hosting.task_name} · ${hostingPhases[hosting.phase]} · 已输入 ${hosting.input_count}/${hosting.max_inputs} 次`;
}

export function TerminalHostingDetails({ hosting }: { hosting: TerminalHosting }) {
  const navigate = useAowTabNavigation();
  const [target, setTarget] = useState<TabTarget>();
  const [error, setError] = useState('');
  const completed = hosting.phase === 'completed';
  const message = hosting.error || (hosting.phase === 'limit_reached' ? '请接管后处理剩余审查结论。' : '');
  const show = Boolean(message) || completed;
  useEffect(() => {
    let active = true;
    setTarget(undefined); setError('');
    if (!show || !hosting.run_id) return;
    void aowApi.projects().then(projects => {
      const worktree = projects.flatMap(project => project.worktrees).find(worktree => worktree.path === hosting.workspace_root);
      if (!worktree) throw new Error('执行记录所属工作区已移除');
      if (active) setTarget({ type: 'automation', workspace: worktree.id, taskId: hosting.task_id, runId: hosting.run_id!, view: 'runs' });
    }).catch(reason => { if (active) setError(reason instanceof Error ? reason.message : String(reason)); });
    return () => { active = false; };
  }, [show, hosting.task_id, hosting.run_id, hosting.workspace_root]);
  if (!show) return null;
  return <div className={`terminal-hosting-details${completed ? ' completed' : ''}`} role={completed && !error ? 'status' : 'alert'}>
    {completed ? <CircleCheck /> : <AlertTriangle />}{(message || error) && <span>{message}{error ? `${message ? ' · ' : ''}${error}` : ''}</span>}
    {target && <a href={tabTargetUrl(target)} onClick={event => {
      event.stopPropagation();
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      void navigate(target).catch(reason => setError(reason instanceof Error ? reason.message : String(reason)));
    }}>查看本次执行</a>}
  </div>;
}
