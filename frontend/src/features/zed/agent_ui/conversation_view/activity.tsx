import { X } from 'lucide-react';
import { ContentBlock, type OpenFile } from './content';
import { visibleContent } from './entry_view_state';
import { list, text, type Data, type ThreadEntry } from '../../types';

export function Compaction({ content, onOpenFile }: { content: Data; onOpenFile: OpenFile }) {
  const summary = list(content.summary).filter(visibleContent);
  const error = text(content.error);
  const label = content.status === 'in_progress' ? '上下文压缩中…' : content.status === 'failed' ? '上下文压缩失败' : content.status === 'completed' ? '上下文已压缩' : ['cancelled', 'canceled'].includes(text(content.status)) ? '上下文压缩已取消' : '上下文压缩';
  if (!summary.length && !error) return <div className="zed-compaction" role="status">{label}</div>;
  return <details className="zed-compaction"><summary>{label}</summary>
    {summary.map((block, index) => <ContentBlock key={index} content={block} onOpenFile={onOpenFile} />)}
    {error && <p className="zed-error" role="alert">{error}</p>}
  </details>;
}

export function Notice({ notice, busy, onDismiss }: { notice: ThreadEntry; busy: boolean; onDismiss: () => void }) {
  const { content } = notice;
  const title = text(content.title) || text(content.message);
  if (!title) return null;
  const severity = text(content.severity) || 'info';
  return <section className="zed-notice" data-severity={severity} role={['error', 'warning'].includes(severity) ? 'alert' : 'status'} aria-label={title}>
    <div className="zed-notice-header"><strong>{title}</strong><button type="button" aria-label={`关闭通知：${title}`} disabled={busy} onClick={onDismiss}><X size={14} /></button></div>
    {!!text(content.description) && <p className="zed-notice-description">{text(content.description)}</p>}
  </section>;
}
