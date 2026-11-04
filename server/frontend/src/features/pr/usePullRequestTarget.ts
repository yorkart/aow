import { t } from '../../lib/i18n';
import { useEffect, useState } from 'react';
import { gitPlatformApi } from '../git-platform-providers/api';
import type { GitPlatformTarget } from '../git-platform-providers/types';

// Remember the selected remote per repository; never reuse a choice for another
// repository, or silently redirect a saved PR to a different provider.
export function usePullRequestTarget(repository: string, visible: boolean) {
  const [state, setState] = useState<{ repository: string; targets: GitPlatformTarget[]; remote: string; error?: string }>();
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const update = () => setRevision(value => value + 1);
    window.addEventListener('aow-git-platform-providers-changed', update);
    return () => window.removeEventListener('aow-git-platform-providers-changed', update);
  }, []);
  useEffect(() => {
    if (!visible) return;
    let active = true;
    setState(undefined);
    void gitPlatformApi.targets(repository, ['pull_requests.list', 'pull_requests.detail', 'pull_requests.diff']).then(targets => {
      if (!active) return;
      let saved = '';
      try { saved = localStorage.getItem('aow-pull-request-remote:' + repository) ?? ''; } catch { /* Storage can be unavailable. */ }
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
    try { localStorage.setItem('aow-pull-request-remote:' + repository, remote); } catch { /* Optional persistence. */ }
    setState(value => value ? { ...value, remote } : value);
  };
  const target = current?.targets.find(t => t.remote === current.remote);
  return { targets: current?.targets ?? [], remote: current?.remote ?? '', target, select,
    ready: !!current && !current.error && (current.targets.length === 0 || !!target),
    error: current?.error ?? (current?.targets.length && !target ? t("pr.choose_a_remote_to_query") : ''),
    reload: () => setRevision(value => value + 1) };
}
