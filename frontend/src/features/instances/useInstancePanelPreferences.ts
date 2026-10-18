import { useEffect, useState } from 'react';
import { persist, readStored } from '../../aow/floatingWorkspaceState';
import type { InstanceGroup, InstanceScopes, InstanceSorts } from './InstancePanelGroups';
import type { InstanceSort } from './InstanceScopeMenu';

export function useInstancePanelPreferences(projectId: string, kind: 'terminal' | 'acp') {
  const scopeKey = `aow-project-${kind}-scope:${projectId}`;
  const sortKey = `aow-project-${kind}-sort:${projectId}`;
  const [scopes, setScopes] = useState<InstanceScopes>(() => {
    const stored = readStored<Partial<InstanceScopes>>(scopeKey, {});
    return { user: stored.user !== false, cli: stored.cli !== false };
  });
  const [sorts, setSorts] = useState<InstanceSorts>(() => {
    const stored = readStored<Partial<InstanceSorts>>(sortKey, {});
    return { user: stored.user === 'createdAt' ? 'createdAt' : 'branch', cli: stored.cli === 'createdAt' ? 'createdAt' : 'branch' };
  });
  useEffect(() => persist(scopeKey, scopes), [scopeKey, scopes]);
  useEffect(() => persist(sortKey, sorts), [sortKey, sorts]);
  return { scopes, sorts,
    setScope: (group: InstanceGroup, value: boolean) => setScopes(current => ({ ...current, [group]: value })),
    setSort: (group: InstanceGroup, value: InstanceSort) => setSorts(current => ({ ...current, [group]: value })),
  };
}
