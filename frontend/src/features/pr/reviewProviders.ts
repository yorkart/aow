import { useEffect, useState } from 'react';
import { prApi } from './api';
import type { ReviewTarget } from './types';

// Remember the selected remote per repository; never reuse a choice for another
// repository, or silently redirect a saved PR to a different provider.
export function useReviewTarget(repository: string, visible: boolean) {
  const [state, setState] = useState<{ repository: string; targets: ReviewTarget[]; remote: string; error?: string }>();
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const update = () => setRevision(value => value + 1);
    window.addEventListener('aow-review-providers-changed', update);
    return () => window.removeEventListener('aow-review-providers-changed', update);
  }, []);
  useEffect(() => {
    if (!visible) return;
    let active = true;
    setState(undefined);
    void prApi.reviewTargets(repository).then(targets => {
      if (!active) return;
      let saved = '';
      try { saved = localStorage.getItem('aow-review-remote:' + repository) ?? ''; } catch { /* Storage can be unavailable. */ }
      const unique = new Set(targets.map(t => `${t.provider}:${t.host}/${t.repository}`));
      const remote = targets.some(t => t.remote === saved) ? saved : unique.size === 1 ? targets[0].remote : '';
      setState({ repository, targets, remote });
    }).catch((reason: unknown) => {
      if (active) setState({ repository, targets: [], remote: '', error: reason instanceof Error ? reason.message : String(reason) });
    });
    return () => { active = false; };
  }, [repository, visible, revision]);
  const current = state?.repository === repository ? state : undefined;
  const select = (remote: string) => {
    try { localStorage.setItem('aow-review-remote:' + repository, remote); } catch { /* Optional persistence. */ }
    setState(value => value ? { ...value, remote } : value);
  };
  const target = current?.targets.find(t => t.remote === current.remote);
  return { targets: current?.targets ?? [], remote: current?.remote ?? '', target, select,
    ready: !!current && !current.error && (current.targets.length === 0 || !!target),
    error: current?.error ?? (current?.targets.length && !target ? '请选择要查询的 remote。' : ''),
    reload: () => setRevision(value => value + 1) };
}
