import type { AutomationTask } from './types';

export function AutomationTaskBadge({ task }: { task: AutomationTask }) {
  const state = task.scheduler_error ? 'failed' : task.is_running ? 'running' : task.enabled ? 'enabled' : 'paused';
  return <span className={`automation-badge ${state}`}><i />{task.scheduler_error ? '配置异常' : task.is_running ? '执行中' : task.kind === 'manual' ? '手动' : task.enabled ? '已启用' : '已暂停'}</span>;
}
