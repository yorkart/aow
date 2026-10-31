import { instanceKey, type InstanceInfo } from '../instances/types';
import type { TaskResource, WorkTask } from './api';

// Runtime descendants inherit organization through their existing instance parents.
// Task dependency edges are deliberately absent from this calculation.
export function taskMembership(resources: TaskResource[], instances: InstanceInfo[], excluded = new Set<string>()): Set<string> {
  const members = new Set(resources.map(instanceKey));
  let changed = true;
  while (changed) {
    changed = false;
    for (const instance of instances) {
      const key = instanceKey(instance);
      if (excluded.has(key) || instance.kind === 'terminal' && instance.tab_id && excluded.has(`terminal:${instance.tab_id}`)) continue;
      if (members.has(key)) continue;
      if (instance.parent && members.has(instanceKey(instance.parent))
        || instance.kind === 'terminal' && instance.tab_id && members.has(`terminal:${instance.tab_id}`)) {
        members.add(key);
        if (instance.kind === 'terminal' && instance.tab_id) members.add(`terminal:${instance.tab_id}`);
        changed = true;
      }
    }
  }
  return members;
}

export function taskResourceOwners(tasks: WorkTask[], instances: InstanceInfo[]): Map<string, string> {
  const owners = new Map(tasks.flatMap(task => task.resources.map(resource => [instanceKey(resource), task.id] as const)));
  const roots = new Map(owners);
  for (const task of tasks) {
    const excluded = new Set([...roots].filter(([, owner]) => owner !== task.id).map(([key]) => key));
    for (const key of taskMembership(task.resources, instances, excluded)) owners.set(key, task.id);
  }
  return owners;
}
