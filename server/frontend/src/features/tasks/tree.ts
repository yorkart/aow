import type { WorkTask } from './api';
export interface TaskNode { task: WorkTask; children: TaskNode[] }
export function taskTree(tasks: WorkTask[]): TaskNode[] {
  const byId = new Map(tasks.map(task => [task.id, task]));
  const children = new Map<string, WorkTask[]>();
  const roots: WorkTask[] = [];
  for (const task of tasks) {
    const parent = task.source_task_id ? byId.get(task.source_task_id) : undefined;
    if (parent && !parent.is_orchestrator && parent.id !== task.id) children.set(parent.id, [...(children.get(parent.id) ?? []), task]);
    else roots.push(task);
  }
  const visited = new Set<string>();
  const visit = (task: WorkTask): TaskNode[] => {
    if (visited.has(task.id)) return [];
    visited.add(task.id);
    return [{ task, children: (children.get(task.id) ?? []).flatMap(visit) }];
  };
  return [...roots, ...tasks].flatMap(visit);
}
