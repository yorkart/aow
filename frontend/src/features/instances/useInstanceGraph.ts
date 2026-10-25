import { localizedFetch } from '../../lib/localizedFetch';
import { t } from '../../lib/i18n';
import { useCallback, useEffect, useState } from 'react';
import { appUrl } from '../../lib/basePath';
import { subscribeWorkspaceChanges } from '../../lib/workspaceEvents';
import type { InstanceInfo } from './types';

export function useInstanceGraph(active = true) {
  const [items, setItems] = useState<InstanceInfo[]>([]);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const reload = useCallback(() => setRevision(value => value + 1), []);
  useEffect(() => {
    if (!active) return;
    const controller = new AbortController();
    let pending = false;
    let queued = false;
    const refresh = async () => {
      if (pending) { queued = true; return; }
      if (controller.signal.aborted || document.visibilityState !== 'visible') return;
      pending = true;
      try {
        const response = await localizedFetch(appUrl('/api/instances'), { signal: controller.signal });
        if (!response.ok) throw new Error(t("instances.unable_to_load_instance_creation_relationships"));
        const result: { items?: InstanceInfo[] } = await response.json();
        if (!controller.signal.aborted) { setItems(result.items ?? []); setError(''); }
      } catch (reason) {
        if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
      } finally {
        pending = false;
        if (queued && !controller.signal.aborted) { queued = false; void refresh(); }
      }
    };
    void refresh();
    const unsubscribe = subscribeWorkspaceChanges(change => { if (change.reset || change.instances || change.terminals) void refresh(); });
    document.addEventListener('visibilitychange', refresh);
    return () => { controller.abort(); unsubscribe(); document.removeEventListener('visibilitychange', refresh); };
  }, [active, revision]);
  return { items, error, reload };
}
