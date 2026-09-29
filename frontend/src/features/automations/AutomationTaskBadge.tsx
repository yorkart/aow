import type { AutomationTask } from './types';
import { AowListRowStatus } from '../../components/AowListRowMeta';

export function AutomationTaskBadge({ task, inList = false }: { task: AutomationTask; inList?: boolean }) {
  const state = task.scheduler_error ? 'failed' : task.is_running ? 'running' : task.enabled ? 'enabled' : 'paused';
  const label = task.scheduler_error ? '配置异常' : task.is_running ? '执行中' : task.kind === 'manual' ? '手动' : task.enabled ? '已启用' : '已暂停';
  return inList ? <AowListRowStatus tone={state === 'failed' ? 'error' : state === 'paused' ? 'muted' : 'success'}>{label}</AowListRowStatus>
    : <span className={`automation-badge ${state}`}><i />{label}</span>;
}
