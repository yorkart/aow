import type { ConfirmationOptions } from '../../components/ConfirmationDialog';
import type { AutomationTask } from './types';

export function deleteTaskConfirmation(task: AutomationTask): ConfirmationOptions {
  return {
    title: task.kind === 'manual' ? '删除手动任务' : '删除自动化',
    description: `删除“${task.name}”${task.kind === 'manual' ? '' : '并移除其运行计划'}？执行历史和已创建的工作区将保留。`,
    confirmLabel: '删除',
    danger: true,
  };
}
