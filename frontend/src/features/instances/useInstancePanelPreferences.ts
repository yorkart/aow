import { useEffect, useState } from 'react';
import { persist, readStored } from '../../aow/floatingWorkspaceState';
import type { InstanceSort } from './InstanceScopeMenu';

export function useInstancePanelPreferences(projectId: string, kind: 'terminal' | 'acp') {
  const scopeKey = `aow-project-${kind}-scope:${projectId}`;
  const sortKey = `aow-project-${kind}-sort:${projectId}`;
  const [showAll, setShowAll] = useState(() => {
    const stored = readStored<boolean | { user?: boolean; cli?: boolean }>(scopeKey, true);
    // Merge legacy source-specific preferences so previously visible worktrees stay visible.
    return typeof stored === 'boolean' ? stored : stored?.user !== false || stored?.cli !== false;
  });
  const [sortBy, setSortBy] = useState<InstanceSort>(() => {
    const stored = readStored<InstanceSort | { user?: InstanceSort; cli?: InstanceSort }>(sortKey, 'branch');
    return stored === 'createdAt' || typeof stored === 'object' && (stored?.user === 'createdAt' || stored?.cli === 'createdAt') ? 'createdAt' : 'branch';
  });
  useEffect(() => persist(scopeKey, showAll), [scopeKey, showAll]);
  useEffect(() => persist(sortKey, sortBy), [sortKey, sortBy]);
  return { showAll, sortBy, setShowAll, setSortBy };
}
