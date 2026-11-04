import { useEffect, useState } from 'react';
import { Archive, Check, ExternalLink, Inbox, RefreshCw } from 'lucide-react';
import { t, getLanguage } from '../../lib/i18n';
import type { AowProject } from '../../aow/types';
import { useAowTabNavigation } from '../../aow/tabNavigation';
import { mentionPreview } from '../agent-roles/mentions';
import { inboxApi, subscribeInbox, type InboxEvent, type InboxPage } from './api';
import './inbox.css';

const message = (error: unknown) => error instanceof Error ? error.message : String(error);

function InboxRow({ event, project }: { event: InboxEvent; project?: string }) {
  const navigate = useAowTabNavigation();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const act = async (action: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true); setError('');
    try { await action(); } catch (error) { setError(message(error)); } finally { setBusy(false); }
  };
  return <article className="inbox-row" data-unread={!event.read}>
    <header><strong>{mentionPreview(event.message.split('\n')[0])}</strong>
      <time dateTime={new Date(event.created_at).toISOString()}>{new Date(event.created_at).toLocaleString(getLanguage())}</time></header>
    <div className="inbox-meta">{project ?? event.project_id} · <span>{t('inbox.' + event.kind)}</span>
      {event.delivery && <span> · {t('inbox.delivery_' + event.delivery.state)}</span>}</div>
    <p className="inbox-summary">{event.message}</p>
    {event.delivery?.error && <p className="inbox-error">{t('inbox.delivery_error')}: {event.delivery.error}</p>}
    <details onToggle={eventToggle => { if (eventToggle.currentTarget.open && !event.read) void act(() => inboxApi.update(event.id, { read: true })); }}>
      <summary>{t('inbox.details')}</summary>
      {event.source?.request && <><h4>{t('inbox.original_request')}</h4><pre>{event.source.request}</pre></>}
      <small>{t('inbox.event')}: {event.id}</small>
    </details>
    <div className="inbox-actions">
      <button disabled={busy} onClick={() => void act(() => navigate({ type: 'acp', thread_id: event.thread_id }))}><ExternalLink size={14} />{t('inbox.open_task')}</button>
      {event.target && <button disabled={busy} onClick={() => void act(() => navigate({ type: 'acp', thread_id: event.target!.thread_id }))}>{t('inbox.open_source')}</button>}
      <button disabled={busy} onClick={() => void act(() => inboxApi.update(event.id, { read: !event.read }))}><Check size={14} />{t(event.read ? 'inbox.mark_unread' : 'inbox.mark_read')}</button>
      <button disabled={busy} onClick={() => void act(() => inboxApi.update(event.id, { archived: !event.archived }))}><Archive size={14} />{t(event.archived ? 'inbox.restore' : 'inbox.archive')}</button>
      {event.delivery?.state === 'blocked' && <button disabled={busy} onClick={() => void act(() => inboxApi.retry(event.id))}><RefreshCw size={14} />{t('inbox.retry')}</button>}
    </div>
    {error && <p className="inbox-error" role="alert">{error}</p>}
  </article>;
}

export function InboxPanel({ projects }: { projects: AowProject[] }) {
  const [project, setProject] = useState('');
  const [filter, setFilter] = useState('all');
  const [offset, setOffset] = useState(0);
  const [revision, setRevision] = useState(0);
  const [page, setPage] = useState<InboxPage>();
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(true);
  useEffect(() => subscribeInbox(() => { setOffset(0); setRevision(value => value + 1); }), []);
  useEffect(() => {
    const controller = new AbortController();
    setLoading(true); setError('');
    const params = new URLSearchParams({ offset: String(offset), limit: '30', unread: String(filter === 'unread'), archived: String(filter === 'archived') });
    if (project) params.set('project_id', project);
    void inboxApi.list(params, controller.signal).then(value => { if (!controller.signal.aborted) setPage(value); })
      .catch(error => { if (!controller.signal.aborted) setError(message(error)); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [project, filter, offset, revision]);
  return <section className="inbox-panel" aria-label={t('inbox.title')} aria-busy={loading}>
    <header className="inbox-toolbar"><h2><Inbox size={19} />{t('inbox.title')}{page ? <span className="inbox-badge">{page.unread}</span> : null}</h2>
      <select aria-label={t('inbox.project')} value={project} onChange={event => { setProject(event.target.value); setOffset(0); }}>
        <option value="">{t('inbox.all_projects')}</option>{projects.filter(project => !project.builtin).map(project => <option key={project.id} value={project.id}>{project.name}</option>)}
      </select>
      <select aria-label={t('inbox.filter')} value={filter} onChange={event => { setFilter(event.target.value); setOffset(0); }}>
        <option value="all">{t('inbox.all')}</option><option value="unread">{t('inbox.unread')}</option><option value="archived">{t('inbox.archived')}</option>
      </select>
      <button aria-label={t('inbox.refresh')} disabled={loading} onClick={() => setRevision(value => value + 1)}><RefreshCw size={16} /></button>
    </header>
    {error && <p className="inbox-error" role="alert">{error}</p>}
    {!error && !loading && !page?.items.length && <p className="inbox-empty">{t('inbox.empty')}</p>}
    <div className="inbox-list">{page?.items.map(event => <InboxRow key={event.id} event={event} project={projects.find(project => project.id === event.project_id)?.name} />)}</div>
    {(offset > 0 || page?.has_more) && <footer className="inbox-pagination">
      <button disabled={loading || offset === 0} onClick={() => setOffset(value => Math.max(0, value - 30))}>{t('inbox.previous')}</button>
      <button disabled={loading || !page?.has_more} onClick={() => setOffset(value => value + 30)}>{t('inbox.next')}</button>
    </footer>}
  </section>;
}
