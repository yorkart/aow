import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';
import type { InstanceInfo } from '../instances/types';
import { taskResourceOwners } from './membership';
import { notifyTasksChanged, tasksApi, type TaskResource, type WorkTask } from './api';

interface TaskScope {
  task: WorkTask;
  tasks: WorkTask[];
  openTask?: (task: WorkTask) => void;
  owns: (kind: TaskResource['kind'], id: string) => boolean;
  owner: (kind: TaskResource['kind'], id: string) => string | undefined;
  remember: (kind: TaskResource['kind'], id: string) => void;
  attach: (kind: TaskResource['kind'], id: string) => Promise<void>;
}
const Context = createContext<TaskScope | undefined>(undefined);
export function TaskScopeProvider({ task, tasks, instances, onOpenTask, children }: { task: WorkTask; tasks: WorkTask[]; instances: InstanceInfo[]; onOpenTask?: (task: WorkTask) => void; children: ReactNode }) {
  const [created, setCreated] = useState<TaskResource[]>([]);
  const owners = useMemo(() => taskResourceOwners(tasks.map(item => item.id === task.id ? { ...task, resources: [...task.resources, ...created] } : item), instances), [tasks, task, created, instances]);
  const owner = useCallback((kind: TaskResource['kind'], id: string) => owners.get(`${kind}:${id}`), [owners]);
  const owns = useCallback((kind: TaskResource['kind'], id: string) => owner(kind, id) === task.id, [owner, task.id]);
  const remember = useCallback((kind: TaskResource['kind'], id: string) => {
    setCreated(items => items.some(item => item.kind === kind && item.id === id) ? items : [...items, { kind, id }]);
    notifyTasksChanged();
  }, []);
  const attach = useCallback(async (kind: TaskResource['kind'], id: string) => {
    if (owns(kind, id)) return;
    await tasksApi.attach(task.id, { kind, id });
    remember(kind, id);
  }, [task.id, owns, remember]);
  return <Context.Provider value={{ task, tasks, openTask: onOpenTask, owns, owner, remember, attach }}>{children}</Context.Provider>;
}
export const useTaskScope = () => useContext(Context);
