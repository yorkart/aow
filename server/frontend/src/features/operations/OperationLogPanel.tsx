import { t, getLanguage } from '../../lib/i18n';
import { appUrl } from '../../lib/basePath';
import { useEffect, useRef, useState } from 'react';
import { ArrowLeft, ChevronLeft, ChevronRight, FileClock, LoaderCircle, RefreshCw, Search, X } from 'lucide-react';
import { type LogCursor, type Operation, type OperationLogPage, operationOutcomeLabels, readOperationLogs } from './operations';
import './operations.css';

export function OperationLogPanel({ initialOperationId, operations, revision, bootId, logError, onClose }: {
  initialOperationId?: string; operations: Operation[]; revision: number; bootId: string; logError: string | null; onClose: () => void;
}) {
  const [filters, setFilters] = useState<Record<string, string>>({ operation_id: initialOperationId ?? '', query: '', kind: '', source: '', level: '' });
  const [query, setQuery] = useState('');
  const [cursors, setCursors] = useState<(LogCursor | undefined)[]>([undefined]);
  const [pageIndex, setPageIndex] = useState(0);
  const [page, setPage] = useState<OperationLogPage>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [reload, setReload] = useState(0);
  const [newRecords, setNewRecords] = useState(false);
  const [height, setHeight] = useState(320);
  const resize = useRef<{ y: number; height: number } | undefined>(undefined);
  const list = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLElement>(null);
  useEffect(() => { panel.current?.focus({ preventScroll: true }); }, []);
  const filterKey = JSON.stringify(filters);
  const cursorKey = JSON.stringify(cursors[pageIndex]);
  const previousRevision = useRef(revision);
  useEffect(() => {
    if (previousRevision.current !== revision) setNewRecords(true);
    previousRevision.current = revision;
  }, [revision]);
  useEffect(() => {
    const controller = new AbortController();
    setLoading(true); setError('');
    void readOperationLogs(filters, cursors[pageIndex], controller.signal).then(result => {
      if (controller.signal.aborted) return;
      setPage(result);
      list.current?.scrollTo({ top: 0 });
    }).catch(reason => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
    }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [filterKey, cursorKey, reload]);
  const changeFilters = (change: Record<string, string>) => {
    setFilters(current => ({ ...current, ...change }));
    setCursors([undefined]); setPageIndex(0); setPage(undefined); setNewRecords(false);
  };
  const refresh = () => { setCursors([undefined]); setPageIndex(0); setReload(value => value + 1); setNewRecords(false); };
  const operation = operations.find(operation => operation.id === filters.operation_id);
  const latest = page?.items[0];
  const sourceLabel = (value: string) => ({ web: t("operations.page"), cli: 'CLI', automation: t("aow.automations"), system: t("operations.system"), auth: t("operations.authentication") }[value] ?? value);
  const resizeTo = (value: number) => setHeight(Math.max(180, Math.min(window.innerHeight * 0.75, value)));
  return <section ref={panel} tabIndex={-1} className="operation-log-panel" aria-label={t("operations.operation_logs")} style={{ height }} onKeyDown={event => {
    if (event.key === 'Escape') { event.stopPropagation(); onClose(); }
  }}>
    <div className="operation-log-resize" role="separator" aria-label={t("operations.resize_operation_logs_height")} aria-orientation="horizontal" tabIndex={0}
      onPointerDown={event => { resize.current = { y: event.clientY, height }; event.currentTarget.setPointerCapture(event.pointerId); }}
      onPointerMove={event => { if (resize.current) resizeTo(resize.current.height + resize.current.y - event.clientY); }}
      onPointerUp={() => { resize.current = undefined; }} onPointerCancel={() => { resize.current = undefined; }}
      onKeyDown={event => { if (event.key === 'ArrowUp' || event.key === 'ArrowDown') { event.preventDefault(); resizeTo(height + (event.key === 'ArrowUp' ? 20 : -20)); } }} />
    <header><div><FileClock size={15} /><strong>{t("operations.operation_logs")}</strong>{filters.operation_id && <button onClick={() => changeFilters({ operation_id: '' })}><ArrowLeft size={13} />{t("operations.all_operations")}</button>}</div>
      <div>{newRecords && <button onClick={refresh}>{t("operations.new_records")}</button>}<button aria-label={t("operations.refresh_operation_logs")} title={t("files.refresh")} onClick={refresh}><RefreshCw size={14} /></button><button aria-label={t("operations.close_operation_logs")} onClick={onClose}><X size={15} /></button></div>
    </header>
    <form className="operation-log-filters" onSubmit={event => { event.preventDefault(); changeFilters({ query: query.trim() }); }}>
      <label><Search size={13} /><input aria-label={t("operations.search_operation_logs")} placeholder={t("operations.search_description_or_resource")} value={query} maxLength={1024} onChange={event => setQuery(event.target.value)} /></label>
      <button type="submit">{t("operations.search")}</button>
      <select aria-label={t("operations.operation_type")} value={filters.kind} onChange={event => changeFilters({ kind: event.target.value })}><option value="">{t("operations.all_types")}</option><option value="worktree.create">{t("aow.create_worktree")}</option><option value="worktree.remove">{t("aow.delete_worktree")}</option><option value="agent.create">{t("operations.create_agent")}</option><option value="auth.login">{t("auth.sign_in")}</option><option value="auth.access">{t("operations.unauthenticated_access")}</option><option value="auth.notification">{t("operations.authentication_notification")}</option></select>
      <select aria-label={t("operations.operation_source")} value={filters.source} onChange={event => changeFilters({ source: event.target.value })}><option value="">{t("operations.all_sources")}</option><option value="web">{t("operations.page")}</option><option value="cli">CLI</option><option value="automation">{t("aow.automations")}</option><option value="system">{t("operations.system")}</option><option value="auth">{t("operations.authentication")}</option></select>
      <select aria-label={t("operations.log_level")} value={filters.level} onChange={event => changeFilters({ level: event.target.value })}><option value="">{t("operations.all_levels")}</option><option value="info">{t("operations.info")}</option><option value="warn">{t("operations.warning")}</option><option value="error">{t("operations.error")}</option></select>
    </form>
    {operation && <div className="operation-log-current">{!operation.outcome && <LoaderCircle size={13} className="spinning" />}<strong>{operation.title} · {operation.outcome ? operationOutcomeLabels[operation.outcome] : t("components.in_progress")}</strong><span>{operation.message}</span>
      {operation.kind === 'agent.create' && operation.resource?.startsWith('/aow/tabs/terminal/') && <a href={appUrl(operation.resource)}>{t("operations.view_terminal")}</a>}
    </div>}
    {pageIndex === 0 && filters.operation_id && !operation && latest && latest.boot_id !== bootId && latest.event !== 'finished' && !latest.outcome && !filters.query && !filters.kind && !filters.source && !filters.level && <div className="operation-log-warning">{t("operations.no_completion_record_found_for_this_operation_the_result")}</div>}
    {(error || logError) && <div className="operation-log-error" role="alert">{error || logError}<button onClick={refresh}>{t("files.refresh")}</button></div>}
    <div className="operation-log-rows" ref={list} aria-busy={loading}>
      {loading && <div className="operation-log-empty"><LoaderCircle size={14} className="spinning" />{t("operations.reading")}</div>}
      {!loading && page?.items.map((record, index) => <article className={`operation-log-row ${record.level}`} key={`${record.operation_id}:${record.timestamp}:${index}`}>
        <time title={record.timestamp}>{new Date(record.timestamp).toLocaleString(getLanguage())}</time><span className="operation-log-source">{sourceLabel(record.source)}</span>
        <button title={t("operations.view_all_logs_for_this_operation")} onClick={() => { setQuery(''); changeFilters({ operation_id: record.operation_id, query: '', kind: '', source: '', level: '' }); }}>{record.title}</button>
        <span className="operation-log-message">{record.message}</span>
        {record.outcome && <span className="operation-log-outcome">{operationOutcomeLabels[record.outcome]}</span>}
      </article>)}
      {!loading && page && !page.items.length && !error && <div className="operation-log-empty">{page.next_cursor ? t("operations.no_matching_records_in_this_section_continue_searching") : t("operations.no_matching_log_records")}</div>}
    </div>
    <footer><span>{page?.budget_exhausted ? t("operations.some_logs_have_been_searched_continue_to_search_earlier") : t("operations.page_2", { value0: pageIndex + 1, value1: page ? t("common.records", { value0: page.items.length }) : '' })}</span>
      <div><button disabled={loading || pageIndex === 0} onClick={() => setPageIndex(index => index - 1)}><ChevronLeft size={14} />{t("operations.previous_page")}</button>
        <button disabled={loading || !!error || !page?.next_cursor} onClick={() => {
          if (page?.next_cursor) { setCursors(current => [...current.slice(0, pageIndex + 1), page.next_cursor!]); setPageIndex(index => index + 1); }
        }}>{page?.budget_exhausted ? t("operations.continue_searching") : t("operations.earlier_records")}<ChevronRight size={14} /></button></div>
    </footer>
  </section>;
}
