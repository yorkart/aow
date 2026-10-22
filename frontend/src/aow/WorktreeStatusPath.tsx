import { t } from '../lib/i18n';
import { useEffect, useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { copyText } from '../lib/clipboard';

export function WorktreeStatusPath({ path }: { path?: string }) {
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState('');
  const [copying, setCopying] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1500);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const label = error || (copied ? t("aow.workspace_path_copied") : t("aow.copy_workspace_path"));
  return <span className="project-aow-status-path">
    <span title={path}>{path ?? t("workspaces.no_active_worktree")}</span>
    {path && <button type="button" title={label} aria-label={label} disabled={copying} data-copied={copied} onClick={async () => {
      setCopying(true);
      try { await copyText(path); setCopied(true); setError(''); }
      catch { setError(t("aow.copy_failed_try_again")); }
      finally { setCopying(false); }
    }}>{copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}</button>}
  </span>;
}
