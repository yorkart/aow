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
  const sourceLabel = (value: string) => ({ web: '页面', cli: 'CLI', automation: '自动化', system: '系统' }[value] ?? value);
  const resizeTo = (value: number) => setHeight(Math.max(180, Math.min(window.innerHeight * 0.75, value)));
  return <section ref={panel} tabIndex={-1} className="operation-log-panel" aria-label="操作日志" style={{ height }} onKeyDown={event => {
    if (event.key === 'Escape') { event.stopPropagation(); onClose(); }
  }}>
    <div className="operation-log-resize" role="separator" aria-label="调整操作日志高度" aria-orientation="horizontal" tabIndex={0}
      onPointerDown={event => { resize.current = { y: event.clientY, height }; event.currentTarget.setPointerCapture(event.pointerId); }}
      onPointerMove={event => { if (resize.current) resizeTo(resize.current.height + resize.current.y - event.clientY); }}
      onPointerUp={() => { resize.current = undefined; }} onPointerCancel={() => { resize.current = undefined; }}
      onKeyDown={event => { if (event.key === 'ArrowUp' || event.key === 'ArrowDown') { event.preventDefault(); resizeTo(height + (event.key === 'ArrowUp' ? 20 : -20)); } }} />
    <header><div><FileClock size={15} /><strong>操作日志</strong>{filters.operation_id && <button onClick={() => changeFilters({ operation_id: '' })}><ArrowLeft size={13} />全部操作</button>}</div>
      <div>{newRecords && <button onClick={refresh}>有新记录</button>}<button aria-label="刷新操作日志" title="刷新" onClick={refresh}><RefreshCw size={14} /></button><button aria-label="关闭操作日志" onClick={onClose}><X size={15} /></button></div>
    </header>
    <form className="operation-log-filters" onSubmit={event => { event.preventDefault(); changeFilters({ query: query.trim() }); }}>
      <label><Search size={13} /><input aria-label="搜索操作日志" placeholder="搜索描述或资源…" value={query} maxLength={1024} onChange={event => setQuery(event.target.value)} /></label>
      <button type="submit">搜索</button>
      <select aria-label="操作类型" value={filters.kind} onChange={event => changeFilters({ kind: event.target.value })}><option value="">全部类型</option><option value="worktree.remove">删除 Worktree</option><option value="agent.create">创建 Agent</option></select>
      <select aria-label="操作来源" value={filters.source} onChange={event => changeFilters({ source: event.target.value })}><option value="">全部来源</option><option value="web">页面</option><option value="cli">CLI</option><option value="automation">自动化</option><option value="system">系统</option></select>
      <select aria-label="日志级别" value={filters.level} onChange={event => changeFilters({ level: event.target.value })}><option value="">全部级别</option><option value="info">信息</option><option value="warn">警告</option><option value="error">错误</option></select>
    </form>
    {operation && <div className="operation-log-current">{!operation.outcome && <LoaderCircle size={13} className="spinning" />}<strong>{operation.title} · {operation.outcome ? operationOutcomeLabels[operation.outcome] : '进行中'}</strong><span>{operation.message}</span>
      {operation.kind === 'agent.create' && operation.resource?.startsWith('/aow/tabs/terminal/') && <a href={appUrl(operation.resource)}>查看 Terminal</a>}
    </div>}
    {pageIndex === 0 && filters.operation_id && !operation && latest && latest.boot_id !== bootId && latest.event !== 'finished' && !filters.query && !filters.kind && !filters.source && !filters.level && <div className="operation-log-warning">这次操作尚未找到结束记录，结果未确认。</div>}
    {(error || logError) && <div className="operation-log-error" role="alert">{error || logError}<button onClick={refresh}>刷新</button></div>}
    <div className="operation-log-rows" ref={list} aria-busy={loading}>
      {loading && <div className="operation-log-empty"><LoaderCircle size={14} className="spinning" />正在读取…</div>}
      {!loading && page?.items.map((record, index) => <article className={`operation-log-row ${record.level}`} key={`${record.operation_id}:${record.timestamp}:${index}`}>
        <time title={record.timestamp}>{new Date(record.timestamp).toLocaleString()}</time><span className="operation-log-source">{sourceLabel(record.source)}</span>
        <button title="查看本次操作的全部日志" onClick={() => { setQuery(''); changeFilters({ operation_id: record.operation_id, query: '', kind: '', source: '', level: '' }); }}>{record.title}</button>
        <span className="operation-log-message">{record.message}</span>
        {record.outcome && <span className="operation-log-outcome">{operationOutcomeLabels[record.outcome]}</span>}
      </article>)}
      {!loading && page && !page.items.length && !error && <div className="operation-log-empty">{page.next_cursor ? '这部分日志没有匹配记录，可以继续查找。' : '没有匹配的日志记录。'}</div>}
    </div>
    <footer><span>{page?.budget_exhausted ? '已搜索部分日志，可继续查找更早记录。' : `第 ${pageIndex + 1} 页${page ? ` · ${page.items.length} 条记录` : ''}`}</span>
      <div><button disabled={loading || pageIndex === 0} onClick={() => setPageIndex(index => index - 1)}><ChevronLeft size={14} />上一页</button>
        <button disabled={loading || !!error || !page?.next_cursor} onClick={() => {
          if (page?.next_cursor) { setCursors(current => [...current.slice(0, pageIndex + 1), page.next_cursor!]); setPageIndex(index => index + 1); }
        }}>{page?.budget_exhausted ? '继续查找' : '更早记录'}<ChevronRight size={14} /></button></div>
    </footer>
  </section>;
}
