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
  const label = error || (copied ? '已复制工作区路径' : '复制工作区路径');
  return <span className="project-aow-status-path">
    <span title={path}>{path ?? 'No active worktree'}</span>
    {path && <button type="button" title={label} aria-label={label} disabled={copying} data-copied={copied} onClick={async () => {
      setCopying(true);
      try { await copyText(path); setCopied(true); setError(''); }
      catch { setError('复制失败，请重试'); }
      finally { setCopying(false); }
    }}>{copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}</button>}
  </span>;
}
