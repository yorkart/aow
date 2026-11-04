import { t } from '../../lib/i18n';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Check, FileClock, LoaderCircle, TriangleAlert, X } from 'lucide-react';
import { type Operation, operationOutcomeLabels } from './operations';
import './operations.css';

export function OperationStatus({ operations, ready, error, logOpen, onOpenOperation, onOpenLogs, beforeLogs }: {
  beforeLogs?: ReactNode;
  operations: Operation[]; ready: boolean; error: string; logOpen: boolean;
  onOpenOperation: (operation: Operation) => void; onOpenLogs: (operationId?: string) => void;
}) {
  const active = operations.filter(operation => !operation.outcome);
  const [expanded, setExpanded] = useState(false);
  const [notice, setNotice] = useState<Operation>();
  const known = useRef<Set<string> | undefined>(undefined);
  useEffect(() => {
    if (!ready) return;
    const finished = operations.filter(operation => operation.outcome);
    if (known.current) {
      const latest = finished.filter(operation => !known.current!.has(operation.id)).at(-1);
      if (latest) setNotice(latest);
    }
    known.current = new Set(finished.map(operation => operation.id));
  }, [operations, ready]);
  useEffect(() => {
    if (!notice || notice.outcome !== 'succeeded') return;
    const timer = window.setTimeout(() => setNotice(undefined), 4000);
    return () => window.clearTimeout(timer);
  }, [notice]);
  useEffect(() => { if (!active.length) setExpanded(false); }, [active.length]);
  const describe = (operation: Operation) => `${operation.title}${operation.total ? ` · ${operation.completed}/${operation.total}` : ''}`;
  return <div className="operation-status">
    {active.length > 0 && <button type="button" className="operation-status-task" aria-label={active.length === 1 ? describe(active[0]) : t("operations.operations_in_progress", { value0: active.length })}
      title={active.length === 1 ? active[0].message : t("operations.view_running_operations")} onClick={() => {
        if (active.length === 1) onOpenOperation(active[0]); else setExpanded(value => !value);
      }}><LoaderCircle size={12} className="spinning" /><span>{active.length === 1 ? describe(active[0]) : t("operations.operations_in_progress", { value0: active.length })}</span></button>}
    {error && <span className="operation-status-warning" title={error} role="status"><TriangleAlert size={12} aria-label={t("operations.operation_status_error")} /></span>}
    {beforeLogs}
    <button type="button" title={t("operations.operation_logs")} aria-label={t("operations.operation_logs")} aria-expanded={logOpen} onClick={() => onOpenLogs()}><FileClock size={13} /></button>
    {expanded && <div className="operation-task-list" role="dialog" aria-label={t("operations.running_operations")} onKeyDown={event => { if (event.key === 'Escape') setExpanded(false); }}>
      <header><strong>{t("operations.running_operations")}</strong><button aria-label={t("operations.close_task_list")} onClick={() => setExpanded(false)}><X size={14} /></button></header>
      {active.map(operation => <button key={operation.id} onClick={() => { setExpanded(false); onOpenOperation(operation); }}>
        <LoaderCircle size={13} className="spinning" /><div><strong>{describe(operation)}</strong><small>{operation.message}</small></div>
      </button>)}
    </div>}
    {notice && <div className={`operation-notice ${notice.outcome === 'succeeded' ? 'success' : 'warning'}`} role="status">
      {notice.outcome === 'succeeded' ? <Check size={16} /> : <TriangleAlert size={16} />}
      <div><strong>{notice.title} · {operationOutcomeLabels[notice.outcome!]}</strong><p>{notice.message}</p>
        {notice.kind === 'worktree.create' && <button onClick={() => { onOpenOperation(notice); setNotice(undefined); }}>{t("operations.view_progress")}</button>}
        <button onClick={() => { onOpenLogs(notice.id); setNotice(undefined); }}>{t("aow.view_logs")}</button></div>
      <button aria-label={t("operations.dismiss_operation_message")} onClick={() => setNotice(undefined)}><X size={14} /></button>
    </div>}
  </div>;
}
