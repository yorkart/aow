import { t } from '../../../../lib/i18n';
import { useId, useState, type ReactNode } from 'react';
import { AlertCircle, AlertTriangle, ChevronDown, Info, X } from 'lucide-react';
import { ContentBlock, type OpenFile } from './content';
import { visibleContent } from './entry_view_state';
import { list, text, type Data, type ThreadEntry } from '../../types';

export function Compaction({ content, onOpenFile }: { content: Data; onOpenFile: OpenFile }) {
  const summary = list(content.summary).filter(visibleContent);
  const error = text(content.error);
  const label = content.status === 'in_progress' ? t("acp.compacting_context") : content.status === 'failed' ? t("acp.context_compaction_failed") : content.status === 'completed' ? t("acp.context_compacted") : ['cancelled', 'canceled'].includes(text(content.status)) ? t("acp.context_compaction_canceled") : t("acp.context_compaction");
  if (!summary.length && !error) return <div className="acp-compaction" role="status">{label}</div>;
  return <details className="acp-compaction"><summary>{label}</summary>
    {summary.map((block, index) => <ContentBlock key={index} content={block} onOpenFile={onOpenFile} />)}
    {error && <p className="acp-error" role="alert">{error}</p>}
  </details>;
}

export function Notice({ notice, busy = false, onDismiss, children }: { notice: ThreadEntry; busy?: boolean; onDismiss?: () => void; children?: ReactNode }) {
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
      <span className="acp-notice-title">{children ?? title}</span>
      {details && <button type="button" className="acp-notice-toggle" aria-expanded={expanded} aria-controls={detailsId} onClick={() => setExpanded(value => !value)}>
        {expanded ? t("aow.collapse") : t("acp.details")}<ChevronDown size={12} aria-hidden="true" />
      </button>}
      {onDismiss && <button type="button" className="acp-notice-dismiss" aria-label={t("acp.dismiss_notification", { value0: title })} title={t("acp.dismiss_notification_2")} disabled={busy} onClick={onDismiss}><X size={14} /></button>}
    </div>
    {details && <p id={detailsId} className="acp-notice-description" hidden={!expanded}>{details}</p>}
  </section>;
}
