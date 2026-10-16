import { useId, useState } from 'react';
import { AlertCircle, AlertTriangle, ChevronDown, Info, X } from 'lucide-react';
import { ContentBlock, type OpenFile } from './content';
import { visibleContent } from './entry_view_state';
import { list, text, type Data, type ThreadEntry } from '../../types';

export function Compaction({ content, onOpenFile }: { content: Data; onOpenFile: OpenFile }) {
  const summary = list(content.summary).filter(visibleContent);
  const error = text(content.error);
  const label = content.status === 'in_progress' ? '上下文压缩中…' : content.status === 'failed' ? '上下文压缩失败' : content.status === 'completed' ? '上下文已压缩' : ['cancelled', 'canceled'].includes(text(content.status)) ? '上下文压缩已取消' : '上下文压缩';
  if (!summary.length && !error) return <div className="acp-compaction" role="status">{label}</div>;
  return <details className="acp-compaction"><summary>{label}</summary>
    {summary.map((block, index) => <ContentBlock key={index} content={block} onOpenFile={onOpenFile} />)}
    {error && <p className="acp-error" role="alert">{error}</p>}
  </details>;
}

export function Notice({ notice, busy, onDismiss }: { notice: ThreadEntry; busy: boolean; onDismiss: () => void }) {
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  const { content } = notice;
  const title = text(content.title) || text(content.message);
  if (!title) return null;
  const severity = text(content.severity) || 'info';
  const details = text(content.description);
  const Icon = severity === 'error' ? AlertCircle : severity === 'warning' ? AlertTriangle : Info;
  return <section className="acp-notice" data-severity={severity} role={['error', 'warning'].includes(severity) ? 'alert' : 'status'} aria-label={title}>
    <div className="acp-notice-header">
      <Icon className="acp-notice-icon" size={15} aria-hidden="true" />
      <span className="acp-notice-title">{title}</span>
      {details && <button type="button" className="acp-notice-toggle" aria-expanded={expanded} aria-controls={detailsId} onClick={() => setExpanded(value => !value)}>
        {expanded ? '收起' : '详情'}<ChevronDown size={12} aria-hidden="true" />
      </button>}
      <button type="button" className="acp-notice-dismiss" aria-label={`关闭通知：${title}`} title="关闭通知" disabled={busy} onClick={onDismiss}><X size={14} /></button>
    </div>
    {details && <p id={detailsId} className="acp-notice-description" hidden={!expanded}>{details}</p>}
  </section>;
}
