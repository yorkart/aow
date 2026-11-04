import { instanceKey, type InstanceInfo, type InstanceRef } from '../instances/types';

export interface ConversationNode { instance: InstanceInfo; children: ConversationNode[] }

/** Scope selects resources; creation parentage alone determines their hierarchy. */
export function conversationTree(instances: InstanceInfo[], workspace: string, owns?: (kind: InstanceRef['kind'], id: string) => boolean): ConversationNode[] {
  const byId = new Map(instances.map(instance => [instanceKey(instance), instance]));
  const descendants = new Map<string, string[]>();
  for (const instance of byId.values()) {
    if (!instance.parent) continue;
    const parent = instanceKey(instance.parent);
    descendants.set(parent, [...(descendants.get(parent) ?? []), instanceKey(instance)]);
  }
  const scoped = new Set([...byId.values()].filter(instance => owns
    ? owns(instance.kind, instance.id) || instance.kind === 'terminal' && !!instance.tab_id && owns('terminal', instance.tab_id)
    : instance.cwd === workspace).map(instanceKey));
  // Worktree children may execute in another directory. Task ownership already
  // incorporates descendants and respects explicit ownership by another task.
  if (!owns) for (const id of scoped) for (const child of descendants.get(id) ?? []) scoped.add(child);
  const included = new Set([...scoped].filter(id => {
    const instance = byId.get(id)!;
    return instance.kind === 'acp' || !!instance.agent_id;
  }));
  const children = new Map<string, string[]>();
  const roots: string[] = [];
  for (const id of included) {
    let parent = byId.get(id)!.parent;
    const seen = new Set([id]);
    // Ordinary shell panes are not conversations; bridge them without displaying them.
    while (parent && scoped.has(instanceKey(parent)) && !seen.has(instanceKey(parent))) {
      const key = instanceKey(parent);
      if (included.has(key)) break;
      seen.add(key);
      parent = byId.get(key)?.parent ?? null;
    }
    const key = parent && instanceKey(parent);
    if (key && included.has(key) && !seen.has(key)) children.set(key, [...(children.get(key) ?? []), id]);
    else roots.push(id);
  }
  const visited = new Set<string>();
  const visit = (id: string): ConversationNode[] => {
    if (visited.has(id)) return [];
    visited.add(id);
    const nested = (children.get(id) ?? []).flatMap(visit);
    const instance = byId.get(id)!;
    // Deleted ancestors only remain when needed to explain surviving children.
    return instance.deleted && !nested.length ? [] : [{ instance, children: nested }];
  };
  return [...roots, ...included].flatMap(visit);
}

export function searchConversations(nodes: ConversationNode[], search: string): ConversationNode[] {
  const query = search.trim().toLocaleLowerCase();
  if (!query) return nodes;
  return nodes.flatMap(node => {
    if (`${node.instance.title} ${node.instance.agent_id ?? ''}`.toLocaleLowerCase().includes(query)) return [node];
    const children = searchConversations(node.children, query);
    return children.length ? [{ ...node, children }] : [];
  });
}
