import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, Copy, ExternalLink, GitCommitHorizontal, LoaderCircle, X } from 'lucide-react';
import type { GitCommit, GitCommitDetail } from './types';

interface Props {
  commit: GitCommit;
  detail?: GitCommitDetail;
  loading: boolean;
  error?: string;
  anchor: HTMLElement;
  pinned: boolean;
  upstream?: string;
  onEnter: () => void;
  onLeave: () => void;
  onClose: () => void;
}

const margin = 8;
const gap = 8;
const desiredWidth = 430;

function relativeTime(value: string) {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return value;
  const seconds = Math.round((timestamp - Date.now()) / 1000);
  const ranges: [Intl.RelativeTimeFormatUnit, number][] = [
    ['year', 31_536_000], ['month', 2_592_000], ['week', 604_800],
    ['day', 86_400], ['hour', 3_600], ['minute', 60],
  ];
  const formatter = new Intl.RelativeTimeFormat('zh-CN', { numeric: 'auto' });
  for (const [unit, size] of ranges) {
    if (Math.abs(seconds) >= size) return formatter.format(Math.round(seconds / size), unit);
  }
  return formatter.format(seconds, 'second');
}

function absoluteTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'medium' }).format(date);
}

async function copyText(value: string) {
  if (navigator.clipboard?.writeText) { await navigator.clipboard.writeText(value); return; }
  const input = document.createElement('textarea');
  input.value = value; input.style.position = 'fixed'; input.style.opacity = '0';
  document.body.appendChild(input); input.select(); document.execCommand('copy'); input.remove();
}

function shortId(value: string) { return value.slice(0, 8); }

export function CommitDetailPopover({ commit, detail, loading, error, anchor, pinned, upstream, onEnter, onLeave, onClose }: Props) {
  const card = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: margin, top: margin, width: desiredWidth });
  const [copied, setCopied] = useState(false);
  const popoverId = `commit-detail-${commit.id}`;

  const updatePosition = () => {
    if (!anchor.isConnected) { onClose(); return; }
    const rect = anchor.getBoundingClientRect();
    if (rect.bottom < 0 || rect.top > window.innerHeight) { onClose(); return; }
    const maximumWidth = Math.max(1, window.innerWidth - margin * 2);
    const width = Math.min(desiredWidth, maximumWidth);
    const right = window.innerWidth - rect.right - gap - margin;
    const left = rect.left - gap - margin;
    let x = right >= Math.min(320, width) ? rect.right + gap : rect.left - gap - width;
    if (right < Math.min(320, width) && left < Math.min(320, width)) x = rect.right + gap;
    x = Math.max(margin, Math.min(x, window.innerWidth - width - margin));
    const height = card.current?.getBoundingClientRect().height ?? 320;
    const y = Math.max(margin, Math.min(rect.top - 6, window.innerHeight - height - margin));
    setPosition({ left: x, top: y, width });
  };

  useLayoutEffect(updatePosition, [anchor, detail, loading, error]);
  useEffect(() => {
    const update = () => updatePosition();
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);
    return () => { window.removeEventListener('resize', update); window.removeEventListener('scroll', update, true); };
  });
  useEffect(() => { setCopied(false); }, [commit.id]);
  useEffect(() => {
    if (pinned) card.current?.focus({ preventScroll: true });
  }, [pinned, commit.id]);

  const identityChanged = detail && (detail.author.name !== detail.committer.name || detail.author.email !== detail.committer.email || detail.author.date !== detail.committer.date);
  const stats = detail?.stats;

  return createPortal(
    <div
      ref={card}
      id={popoverId}
      className={`commit-detail-popover${pinned ? ' pinned' : ''}`}
      role="dialog"
      aria-label={`提交详情：${commit.subject}`}
      tabIndex={-1}
      style={position}
      onPointerEnter={onEnter}
      onPointerLeave={onLeave}
      onFocusCapture={onEnter}
      onBlurCapture={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) onLeave(); }}
    >
      {pinned ? <button className="commit-detail-close" title="关闭" onClick={onClose}><X /></button> : null}
      {loading ? <div className="commit-detail-loading"><LoaderCircle className="spinning" />正在加载提交详情…</div> : null}
      {error ? <div className="commit-detail-error">{error}</div> : null}
      {detail ? <>
        <div className="commit-detail-author">
          <span className="commit-avatar">{detail.author.name.trim().charAt(0).toUpperCase() || '?'}</span>
          <div><strong>{detail.author.name}</strong><span>{detail.author.email}</span></div>
          <time title={absoluteTime(detail.author.date)}>{relativeTime(detail.author.date)}</time>
        </div>
        <h4>{detail.subject}</h4>
        {detail.body.trim() ? <pre className="commit-detail-body">{detail.body.trim()}</pre> : null}
        {identityChanged ? <div className="commit-detail-committer">提交者：{detail.committer.name} &lt;{detail.committer.email}&gt; · {absoluteTime(detail.committer.date)}</div> : null}
        {stats ? <div className="commit-detail-stats">
          <span>{stats.files_changed} 个文件变更</span>
          <b>+{stats.insertions}</b><em>-{stats.deletions}</em>
          {stats.binary_files ? <span>{stats.binary_files} 个二进制文件</span> : null}
        </div> : null}
        <div className="commit-detail-meta">
          <span className={`commit-sync-state ${commit.is_pushed ? 'pushed' : 'outgoing'}`}>{commit.is_pushed ? `已推送${upstream ? ` · ${upstream}` : ''}` : '仅本地'}</span>
          {detail.refs.map((ref) => <span className="commit-detail-ref" key={ref}>{ref}</span>)}
        </div>
        {detail.parents.length > 1 ? <div className="commit-detail-parents"><span>Parents</span>{detail.parents.map((parent) => <code key={parent}>{shortId(parent)}</code>)}</div> : null}
        <footer className="commit-detail-footer">
          <GitCommitHorizontal /><code title={detail.id}>{detail.id}</code>
          <button title="复制 commit SHA" onClick={() => { void copyText(detail.id).then(() => { setCopied(true); window.setTimeout(() => setCopied(false), 1200); }); }}>{copied ? <Check /> : <Copy />}</button>
          {detail.commit_url ? <a href={detail.commit_url} target="_blank" rel="noopener noreferrer"><ExternalLink />在 {detail.remote_name ?? '远程仓库'} 中打开</a> : null}
        </footer>
      </> : null}
    </div>,
    document.body,
  );
}
