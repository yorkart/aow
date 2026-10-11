import { useId, useState } from 'react';
import { CheckCircle2, ChevronDown, ChevronRight, CircleDashed, LoaderCircle, X } from 'lucide-react';
import { text, type Data } from '../../types';
import { AgentMarkdown } from './markdown';
import type { OpenFile } from './content';

// Summary and row states follow Zed's render_plan_summary/render_plan_entries.
export function Plan({ entries, busy, onClear, onOpenFile }: { entries: Data[]; busy: boolean; onClear: () => void; onOpenFile: OpenFile }) {
  const [expanded, setExpanded] = useState(false);
  const itemsId = useId();
  if (!entries.length) return null;
  const completed = entries.filter(entry => entry.status === 'completed').length;
  const cancelled = entries.filter(entry => entry.status === 'cancelled').length;
  const pending = entries.length - completed - cancelled;
  const current = entries.find(entry => entry.status === 'in_progress');
  const summary = pending === 0 ? cancelled === 0 ? 'All Done' : completed === 0 ? 'Cancelled' : 'Finished'
    : completed === 0 ? `${entries.length} Tasks` : `${completed}/${entries.length}`;
  return <section className="acp-plan" aria-label="Plan">
    <div className="acp-plan-header" onClick={event => {
      if (!(event.target as Element).closest('a')) setExpanded(value => !value);
    }}>
      <button className="acp-plan-toggle" type="button" aria-label={expanded ? '收起计划' : '展开计划'} aria-expanded={expanded} aria-controls={itemsId}>{expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</button>
      {!expanded && current ? <div className="acp-plan-current"><span>Current:</span><AgentMarkdown text={text(current.content)} onOpenFile={onOpenFile} controls={false} /><span className="acp-plan-remaining">{pending} left</span></div>
        : <div className="acp-plan-summary"><span>Plan</span><span>{summary}</span></div>}
      <button className="acp-plan-clear" type="button" aria-label="清除计划" title="Clear Plan" disabled={busy} onClick={event => { event.stopPropagation(); onClear(); }}><X size={12} /></button>
    </div>
    <ol id={itemsId} className="acp-plan-entries" hidden={!expanded}>{entries.map((entry, index) => <li key={index} data-status={text(entry.status)} title={text(entry.content)}>
      {entry.status === 'completed' ? <CheckCircle2 size={14} /> : entry.status === 'in_progress' ? <LoaderCircle size={14} /> : entry.status === 'cancelled' ? <X size={14} /> : <CircleDashed size={14} />}
      <AgentMarkdown text={text(entry.content)} onOpenFile={onOpenFile} controls={false} />
    </li>)}</ol>
  </section>;
}
