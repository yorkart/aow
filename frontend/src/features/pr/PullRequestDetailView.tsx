import { t } from '../../lib/i18n';
import { lazy, Suspense, useCallback, useEffect, useId, useRef, useState } from 'react';
import type { KeyboardEvent, ReactNode } from 'react';
import { ArrowLeft, ArrowRight, Check, ChevronDown, ChevronRight, Circle, CircleAlert, Clock3, ExternalLink, FileDiff, FileText, FileWarning, GitBranch, GitCommitHorizontal, GitMerge, GitPullRequest, LoaderCircle, MessageSquareMore, RefreshCw, Search, ShieldCheck, XCircle } from 'lucide-react';
import { prApi } from './api';
import { pullRequestDiffLines } from './pullRequestDiff';
import { formatPrTime, mergeCheckLabel, safeExternalUrl, stateClass, stateLabel } from './pullRequestPresentation';
import type { PullRequestCheck, PullRequestDetail, PullRequestDiff, PullRequestFile, PullRequestThread, PullRequestUser } from './types';
import { FileTypeIcon } from '../files/FileTypeIcon';
import { MarkdownContent } from '../../components/MarkdownContent';
import './PullRequestDetailView.css';

const DesktopDiff = lazy(() => import('./PullRequestDesktopDiff'));

interface Props { provider?: string; remote?: string; repository: string; number: number; visible: boolean; mobile?: boolean; onBack?: () => void }
type DetailTab = 'overview' | 'changes' | 'checks' | 'discussion';
type FileDiffState = { loading?: boolean; diff?: PullRequestDiff; error?: string };

function errorMessage(reason: unknown) { return reason instanceof Error ? reason.message : String(reason); }

function StateIcon({ value }: { value: string }) {
  const state = stateClass(value);
  if (state === 'passed') return <Check />;
  if (state === 'failed') return <XCircle />;
  if (['running', 'in_progress'].includes(value.toLowerCase())) return <LoaderCircle className="spinning" />;
  if (state === 'pending') return <Clock3 />;
  return <Circle />;
}

function Badge({ value, children }: { value: string; children?: ReactNode }) {
  return <span className={'pr-badge ' + stateClass(value)}><StateIcon value={value} />{children || stateLabel(value)}</span>;
}

function Person({ user }: { user: PullRequestUser }) {
  const name = user.display_name || user.username;
  return <div className="pr-person"><span className="pr-avatar" aria-hidden="true">{name.slice(0, 1).toUpperCase()}</span><span><strong>{name}</strong><small>@{user.username}</small></span></div>;
}

function Empty({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return <div className="pr-empty">{icon}<span>{children}</span></div>;
}

function InlineFileDiff({ detail, file, state, modelScope, sideBySide, onRetry, mobile }: {
  detail: PullRequestDetail; file: PullRequestFile; state?: FileDiffState; modelScope: string; sideBySide: boolean; onRetry: () => void; mobile: boolean;
}) {
  if (state?.loading) return <Empty icon={<LoaderCircle className="spinning" />}>{t("pr.loading_diff")}</Empty>;
  if (state?.error) return <div className="pr-diff-error" role="alert"><CircleAlert /><span>{state.error}</span><button className="pr-text-button" onClick={onRetry}>{t("aow.retry")}</button></div>;
  const diff = state?.diff;
  if (!diff) return null;
  if (diff.binary) return <Empty icon={<FileWarning />}>{t("pr.binary_file_text_diff_unavailable")}</Empty>;
  if (diff.truncated || (!mobile && (diff.original === null || diff.modified === null))) return <Empty icon={<FileWarning />}>{t("pr.file_is_too_large_or_unavailable_text_diff_unavailable")}</Empty>;
  return <>
    {diff.original_path && diff.original_path !== file.path ? <div className="pr-rename">{t("pr.original_path")}<code>{diff.original_path}</code></div> : null}
    {mobile ? diff.patch == null ? <Empty icon={<FileWarning />}>{t("pr.the_current_server_does_not_provide_unified_diffs_update")}</Empty>
      : diff.patch ? <div className="pr-touch-diff" role="region" tabIndex={0} aria-label={t("pr.unified_diff", { value0: file.path })}>
        <table><thead><tr><th>{t("pr.old_line")}</th><th>{t("pr.new_line")}</th><th aria-label={t("pr.change_type")} /><th>{t("pr.code")}</th></tr></thead><tbody>
          {pullRequestDiffLines(diff.patch).map((line, index) => <tr key={index} className={line.kind}>
            <td className="pr-diff-line-number">{line.oldLine}</td><td className="pr-diff-line-number">{line.newLine}</td><td className="pr-diff-marker">{line.kind === 'added' ? '+' : line.kind === 'removed' ? '−' : ''}</td><td><code>{line.text || ' '}</code></td>
          </tr>)}
        </tbody></table>
      </div> : <Empty icon={<FileDiff />}>{t("pr.no_text_changes_only_the_file_name_or_permissions")}</Empty>
      : <Suspense fallback={<Empty icon={<LoaderCircle className="spinning" />}>{t("pr.loading_diff")}</Empty>}>
        <DesktopDiff diff={diff} number={detail.number} path={file.path} modelScope={modelScope} sideBySide={sideBySide} />
      </Suspense>}
  </>;
}

function CheckCard({ check }: { check: PullRequestCheck }) {
  const value = check.conclusion || check.status;
  const url = safeExternalUrl(check.details_url);
  return <details className="pr-check-card">
    <summary><span className={'pr-check-icon ' + stateClass(value)}><StateIcon value={value} /></span><span className="pr-check-name"><strong>{check.name}</strong>{check.description ? <small>{check.description}</small> : null}</span>{check.required ? <span className="pr-required">{t("pr.required")}</span> : null}<Badge value={value} /><ChevronDown className="pr-disclosure" /></summary>
    <div className="pr-check-content">
      <div className="pr-check-meta"><span>{t("pr.started")} {formatPrTime(check.started_at)}</span><span>{t("pr.completed")} {formatPrTime(check.completed_at)}</span>{url ? <a className="pr-link" href={url} target="_blank" rel="noreferrer">{t("pr.view_check_details")}<ExternalLink /></a> : null}</div>
      {check.text ? <MarkdownContent readOnly text={check.text} className="pr-markdown" /> : <p className="pr-muted">{t("pr.this_check_has_no_detailed_output")}</p>}
    </div>
  </details>;
}

function DiscussionCard({ thread }: { thread: PullRequestThread }) {
  const comments = thread.comments?.length ? thread.comments : [{ id: thread.id, author: thread.author, body: thread.body, created_at: thread.updated_at || '', updated_at: thread.updated_at || '' }];
  return <section className="pr-discussion-card">
    <header><MessageSquareMore /><code title={thread.path || t("pr.general_discussion")}>{thread.path || t("pr.general_discussion")}{thread.line ? `:${thread.line}` : ''}</code><Badge value={thread.status === 'open' ? 'pending' : thread.status}>{thread.status === 'open' ? t("pr.unresolved") : stateLabel(thread.status)}</Badge></header>
    {comments.map((comment, index) => <div className="pr-comment" key={comment.id || index}>
      <span className="pr-avatar" aria-hidden="true">{(comment.author || '?').slice(0, 1).toUpperCase()}</span>
      <div className="pr-comment-main"><div className="pr-comment-byline"><strong>{comment.author || t("pr.unknown_user")}</strong><time dateTime={comment.created_at || undefined}>{formatPrTime(comment.created_at)}</time>{comment.updated_at && comment.updated_at !== comment.created_at ? <span title={formatPrTime(comment.updated_at)}>{t("pr.updated_2")}</span> : null}</div><MarkdownContent readOnly text={comment.body || t("pr.comment_content_unavailable")} className="pr-markdown" /></div>
    </div>)}
  </section>;
}

// Scope caches and Monaco models to this repository/PR, including when a host reuses the component.
export function PullRequestDetailView(props: Props) {
  return <PullRequestContent key={`${props.repository}:${props.provider}:${props.remote}:${props.number}`} {...props} />;
}

function PullRequestContent({ repository, provider, remote, number, visible, mobile = false, onBack }: Props) {
  const [detail, setDetail] = useState<PullRequestDetail>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [activeTab, setActiveTab] = useState<DetailTab>('overview');
  const [fileQuery, setFileQuery] = useState('');
  const [checkQuery, setCheckQuery] = useState('');
  const [descriptionCollapsed, setDescriptionCollapsed] = useState(false);
  const [unresolvedOnly, setUnresolvedOnly] = useState(false);
  const [checkFilter, setCheckFilter] = useState('all');
  const [sideBySide, setSideBySide] = useState(true);
  const [expandedFiles, setExpandedFiles] = useState<Set<string>>(() => new Set());
  const [fileDiffs, setFileDiffs] = useState<Record<string, FileDiffState>>({});
  const requestVersion = useRef(0);
  const requested = useRef(false);
  const diffCache = useRef<Record<string, FileDiffState>>({});
  const scrollRef = useRef<HTMLDivElement>(null);
  const mobileReaderRef = useRef<HTMLElement>(null);
  const prefix = useId();

  const load = useCallback(async () => {
    const version = ++requestVersion.current;
    requested.current = true;
    // Invalidate pending diffs as soon as a new snapshot is requested.
    diffCache.current = {};
    setExpandedFiles(new Set()); setFileDiffs({});
    setLoading(true); setError('');
    try {
      const next = await prApi.myPullRequest(repository, number, { provider, remote });
      if (version === requestVersion.current) setDetail(next);
    } catch (reason) {
      if (version === requestVersion.current) setError(errorMessage(reason));
    } finally {
      if (version === requestVersion.current) setLoading(false);
    }
  }, [number, repository, provider, remote]);

  useEffect(() => { if (visible && !requested.current) void load(); }, [load, visible]);
  useEffect(() => () => { requestVersion.current += 1; requested.current = false; }, []);
  useEffect(() => {
    if (mobile) {
      const reader = mobileReaderRef.current;
      if (reader) reader.scrollTo({ top: Math.min(reader.scrollTop, reader.querySelector<HTMLElement>('.pr-header')?.offsetHeight ?? 0) });
    } else scrollRef.current?.scrollTo({ top: 0 });
  }, [activeTab, mobile]);

  const loadDiff = useCallback((file: PullRequestFile, retry = false) => {
    if (loading || (!retry && diffCache.current[file.path])) return;
    const version = requestVersion.current;
    diffCache.current[file.path] = { loading: true };
    setFileDiffs({ ...diffCache.current });
    void prApi.myPullRequestDiff(repository, number, file.path, mobile, { provider: detail?.provider ?? provider, remote: detail?.remote ?? remote }).then((diff) => {
      if (version !== requestVersion.current) return;
      diffCache.current[file.path] = { diff }; setFileDiffs({ ...diffCache.current });
    }).catch((reason) => {
      if (version !== requestVersion.current) return;
      diffCache.current[file.path] = { error: errorMessage(reason) }; setFileDiffs({ ...diffCache.current });
    });
  }, [loading, mobile, number, repository, provider, remote, detail?.provider, detail?.remote]);

  const toggleFile = (file: PullRequestFile) => {
    const open = !expandedFiles.has(file.path);
    setExpandedFiles((current) => { const next = new Set(current); if (open) next.add(file.path); else next.delete(file.path); return next; });
    if (open) loadDiff(file);
  };

  if (!detail) return <div className="pr-detail-state" role={error ? 'alert' : 'status'}>
    {onBack ? <button className="pr-control pr-state-back" aria-label={t("pr.back_to_pr_list")} onClick={onBack}><ArrowLeft />{t("pr.pr_list")}</button> : null}
    {error ? <><CircleAlert /><strong>{t("pr.unable_to_load_pr")}{number}</strong><p>{error}</p><button className="pr-control" disabled={loading} onClick={() => void load()}><RefreshCw />{t("aow.retry")}</button></> : <><LoaderCircle className="spinning" /><span>{t("pr.loading_pr")}{number}…</span></>}
  </div>;

  const review = detail.review_status || 'unknown';
  const ci = detail.check_summary_status || 'unknown';
  const threads = detail.threads ?? detail.unresolved_threads;
  const discussions = unresolvedOnly ? threads.filter((thread) => thread.status === 'open') : threads;
  const files = detail.files.filter((file) => file.path.toLowerCase().includes(fileQuery.toLowerCase().trim()));
  const checks = detail.checks.filter((check) => (checkFilter === 'all' || stateClass(check.conclusion || check.status) === checkFilter) && check.name.toLowerCase().includes(checkQuery.trim().toLowerCase()));
  const passedChecks = detail.checks.filter((check) => stateClass(check.conclusion || check.status) === 'passed').length;
  const mrUrl = safeExternalUrl(detail.url);
  const repoName = mrUrl ? new URL(mrUrl).pathname.split(/\/(?:pull_requests|pull)\//)[0].replace(/^\//, '') : repository.split('/').filter(Boolean).slice(-2).join('/');
  const additions = detail.files.some((file) => file.additions != null) ? detail.files.reduce((sum, file) => sum + (file.additions || 0), 0) : null;
  const deletions = detail.files.some((file) => file.deletions != null) ? detail.files.reduce((sum, file) => sum + (file.deletions || 0), 0) : null;
  const partialStats = detail.files.some((file) => file.additions == null || file.deletions == null);
  const mergeState = detail.mergeable === true ? 'passed' : detail.mergeable === false ? 'pending' : 'unknown';
  const mergeLabel = detail.status === 'merged' ? t("pr.this_pr_has_been_merged") : detail.status === 'closed' ? t("pr.this_pr_has_been_closed") : detail.mergeable === true ? t("pr.merge_requirements_satisfied") : detail.mergeable === false ? t("pr.merge_requirements_not_satisfied_yet") : t("pr.merge_requirements_unknown");
  const tabs: { id: DetailTab; label: string; icon: ReactNode; count?: number }[] = [
    { id: 'overview', label: t("pr.overview"), icon: <FileText /> },
    { id: 'changes', label: t("pr.changed_files"), icon: <FileDiff />, count: detail.changes_count },
    { id: 'checks', label: t("pr.checks"), icon: <ShieldCheck />, count: detail.checks.length },
    { id: 'discussion', label: t("pr.discussion"), icon: <MessageSquareMore />, count: threads.length },
  ];
  const tabKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let next: number;
    if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
    else if (event.key === 'ArrowLeft') next = (index + tabs.length - 1) % tabs.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = tabs.length - 1;
    else return;
    event.preventDefault(); setActiveTab(tabs[next].id);
    document.getElementById(`${prefix}-${tabs[next].id}-tab`)?.focus();
  };

  return <article ref={mobileReaderRef} className={'pr-detail' + (mobile ? ' pr-detail-mobile' : '')} aria-label={t("pr.pr_details", { value0: number })} aria-busy={loading}>
    <header className="pr-header">
      <div className="pr-breadcrumb">{onBack ? <button className="pr-back-button" aria-label={t("pr.back_to_pr_list")} onClick={onBack}><ArrowLeft /></button> : null}<span title={repository}>{repoName}</span><ChevronRight /><span>{t("navigation.pull_requests_label")}</span><ChevronRight /><strong>#{detail.number}</strong></div>
      <div className="pr-title-row"><h1>{detail.title}</h1><div className="pr-header-actions">{mrUrl ? <a className="pr-control" href={mrUrl} target="_blank" rel="noreferrer" aria-label={t("pr.view_pr_on", { value0: detail.provider_name || t("pr.platform") })}><ExternalLink /><span>{detail.provider_name || t("pr.platform")}</span></a> : null}<button className="pr-control pr-icon-button" title={t("pr.refresh_pr_details")} aria-label={t("pr.refresh_pr_details")} disabled={loading} onClick={() => void load()}><RefreshCw className={loading ? 'spinning' : ''} /></button></div></div>
      <div className="pr-author-line">{detail.author ? <strong>{detail.author.display_name || detail.author.username}</strong> : null}<span>{t("pr.created")} <time dateTime={detail.created_at}>{formatPrTime(detail.created_at)}</time></span></div>
      <div className="pr-context"><span className={'pr-lifecycle ' + detail.status}>{detail.status === 'merged' ? <GitMerge /> : <GitPullRequest />}{stateLabel(detail.status)}</span>{detail.draft ? <span className="pr-draft">{t("pr.draft")}</span> : null}<span className="pr-branch" title={detail.source_branch}><GitBranch /><code>{detail.source_branch}</code></span><ArrowRight className="pr-branch-arrow" /><span className="pr-branch" title={detail.target_branch}><GitBranch /><code>{detail.target_branch}</code></span><span className="pr-commit-count"><GitCommitHorizontal />{detail.commits_count} {t("pr.commits")}</span></div>
      {detail.diverged_commits_count ? <p className="pr-branch-behind">{t("pr.the_current_pr_is_behind_the_target_branch")} {detail.diverged_commits_count} {t("pr.commits")}</p> : null}
    </header>
    <nav className="pr-tabs" role="tablist" aria-label={t("pr.pull_request_content")}>{tabs.map((tab, index) => <button key={tab.id} id={`${prefix}-${tab.id}-tab`} role="tab" tabIndex={activeTab === tab.id ? 0 : -1} aria-selected={activeTab === tab.id} aria-controls={`${prefix}-${tab.id}`} onKeyDown={(event) => tabKeyDown(event, index)} onClick={() => setActiveTab(tab.id)}>{tab.icon}{tab.label}{tab.count != null ? <span className="pr-tab-count">{tab.count}</span> : null}{tab.id === 'discussion' && detail.unresolved_threads.length > 0 ? <i className="pr-attention-dot" title={t("pr.unresolved_discussions", { value0: detail.unresolved_threads.length })} /> : null}</button>)}</nav>
    <div className="pr-scroll" ref={scrollRef}>
      {error ? <div className="pr-notice" role="alert"><CircleAlert /><span>{t("pr.refresh_failed_showing_previously_loaded_content")}{error}</span></div> : null}
      {detail.warnings?.length ? <div className="pr-notice" role="status"><CircleAlert /><div><strong>{t("pr.some_information_was_not_fully_loaded")}</strong>{detail.warnings.map((warning) => <p key={warning}>{warning}</p>)}</div></div> : null}
      <div className={'pr-layout' + (activeTab === 'overview' ? '' : ' pr-layout-wide')}>
        <main className="pr-main">
          <section id={`${prefix}-overview`} role="tabpanel" aria-labelledby={`${prefix}-overview-tab`} hidden={activeTab !== 'overview'}>
            <div className="pr-status-grid">
              <button onClick={() => setActiveTab('checks')}><span><ShieldCheck />{t("pr.code_review")}</span><Badge value={review} /></button>
              <button onClick={() => setActiveTab('checks')}><span><Check />{t("pr.continuous_integration")} <small>{passedChecks}/{detail.checks.length}</small></span><Badge value={ci} /></button>
              <button onClick={() => { setUnresolvedOnly(true); setActiveTab('discussion'); }}><span><MessageSquareMore />{t("pr.unresolved_discussions_2")}</span><strong className={detail.unresolved_threads.length ? 'pr-warning-text' : ''}>{detail.unresolved_threads.length}<small>  {t("pr.items")}</small></strong></button>
            </div>
            <section className="pr-card pr-description"><header><button className="pr-card-toggle" aria-expanded={!descriptionCollapsed} aria-controls={`${prefix}-description`} onClick={() => setDescriptionCollapsed(!descriptionCollapsed)}>{descriptionCollapsed ? <ChevronRight /> : <ChevronDown />}<h2>{t("pr.description")}</h2></button></header><div id={`${prefix}-description`} className="pr-card-body" hidden={descriptionCollapsed}>{detail.description ? <MarkdownContent readOnly text={detail.description} className="pr-markdown" /> : <p className="pr-muted">{t("pr.the_author_has_not_added_a_description")}</p>}</div></section>
            <section className="pr-card pr-merge-card"><header><GitMerge /><h2>{t("pr.merge_requirements")}</h2><Badge value={detail.status === 'merged' ? 'passed' : mergeState}>{mergeLabel}</Badge></header><div className="pr-gates">{detail.merge_checks?.length ? detail.merge_checks.map((check, index) => <div className="pr-gate" key={check.name + index}><span className={'pr-check-icon ' + (check.passed === true ? 'passed' : check.passed === false ? 'pending' : 'neutral')}><StateIcon value={check.passed === true ? 'passed' : check.passed === false ? 'pending' : 'unknown'} /></span><div><strong>{mergeCheckLabel(check.name)}</strong>{check.reason ? <p>{check.reason}</p> : null}</div><small>{check.passed === true ? t("pr.passed") : check.passed === false ? t("pr.not_satisfied") : t("pr.unknown")}</small></div>) : <p className="pr-muted">{t("pr.no_specific_merge_requirements_provided")}</p>}</div></section>
          </section>
          <section id={`${prefix}-changes`} role="tabpanel" aria-labelledby={`${prefix}-changes-tab`} hidden={activeTab !== 'changes'}>
            <div className="pr-section-heading"><div><h2>{t("pr.changed_files")}</h2><span>{detail.changes_count} {t("pr.files")}{additions != null ? <b className="pr-additions">+{additions}</b> : null}{deletions != null ? <b className="pr-deletions">−{deletions}</b> : null}{partialStats && additions != null ? <small>{t("pr.known_line_count")}</small> : null}</span></div>{mobile ? <span className="pr-muted">{t("pr.unified_diff_2")}</span> : <div className="pr-segmented" aria-label={t("pr.diff_display_mode")}><button aria-pressed={sideBySide} onClick={() => setSideBySide(true)}>{t("pr.side_by_side")}</button><button aria-pressed={!sideBySide} onClick={() => setSideBySide(false)}>{t("pr.unified")}</button></div>}</div>
            <div className="pr-file-toolbar"><label className="pr-search"><Search /><input aria-label={t("pr.filter_changed_files")} placeholder={t("pr.filter_by_file_path")} value={fileQuery} onChange={(event) => setFileQuery(event.target.value)} />{fileQuery ? <button aria-label={t("pr.clear_file_filter")} onClick={() => setFileQuery('')}>×</button> : null}</label><span>{files.length} / {detail.files.length}</span><button className="pr-text-button" disabled={!expandedFiles.size} onClick={() => setExpandedFiles(new Set())}>{t("pr.collapse_all")}</button></div>
            <div className="pr-files">{files.length ? files.map((file) => {
              const expanded = expandedFiles.has(file.path);
              const names: Record<string, string> = { added: t("pr.added"), deleted: t("acp.delete"), modified: t("pr.modified"), renamed: t("aow.rename"), copied: t("pr.copy") };
              return <section className={'pr-file' + (expanded ? ' expanded' : '')} key={file.path}>
                <button className="pr-file-header" disabled={loading} aria-expanded={expanded} aria-controls={`${prefix}-file-${encodeURIComponent(file.path)}`} onClick={() => toggleFile(file)}>{expanded ? <ChevronDown /> : <ChevronRight />}<FileTypeIcon path={file.path} /><span className="pr-file-path" title={file.path}>{file.path}</span><span className={'pr-change-type ' + file.change_type}>{names[file.change_type] || file.change_type}</span><span className="pr-file-stat">{file.additions != null ? <b className="pr-additions">+{file.additions}</b> : null}{file.deletions != null ? <b className="pr-deletions">−{file.deletions}</b> : null}</span></button>
                <div id={`${prefix}-file-${encodeURIComponent(file.path)}`} className="pr-inline-diff" hidden={!expanded}>{expanded && activeTab === 'changes' ? <InlineFileDiff detail={detail} file={file} state={fileDiffs[file.path]} mobile={mobile} modelScope={`${repository}:${prefix}`} sideBySide={sideBySide} onRetry={() => loadDiff(file, true)} /> : null}</div>
              </section>;
            }) : <Empty icon={<FileDiff />}>{fileQuery ? t("pr.no_matching_files_try_another_path") : t("pr.no_file_changes_to_display")}</Empty>}</div>
          </section>
          <section id={`${prefix}-checks`} role="tabpanel" aria-labelledby={`${prefix}-checks-tab`} hidden={activeTab !== 'checks'}>
            <div className="pr-section-heading"><div><h2>{t("pr.checks_and_reviews")}</h2><span>{passedChecks} / {detail.checks.length} {t("pr.checks_passed")}</span></div><label className="pr-filter-label">{t("pr.show")}<select aria-label={t("pr.filter_checks")} value={checkFilter} onChange={(event) => setCheckFilter(event.target.value)}><option value="all">{t("pr.all_checks")}</option><option value="failed">{t("pr.failed")}</option><option value="pending">{t("pr.waiting_running")}</option><option value="passed">{t("pr.passed")}</option></select></label></div>
            <label className="pr-search pr-check-search"><Search /><input aria-label={t("pr.search_check_names")} placeholder={t("pr.search_check_names_2")} value={checkQuery} onChange={(event) => setCheckQuery(event.target.value)} /></label>
            <div className="pr-review-summary"><ShieldCheck /><span>{t("pr.code_review")}</span><Badge value={review} /><small>{detail.reviewers.length} {t("pr.reviewers")}</small></div>
            <div className="pr-checks">{checks.length ? checks.map((check, index) => <CheckCard key={check.id || `${check.name}:${index}`} check={check} />) : <Empty icon={<ShieldCheck />}>{detail.checks.length ? t("pr.no_checks_match_the_filters") : t("pr.no_ci_checks")}</Empty>}</div>
          </section>
          <section id={`${prefix}-discussion`} role="tabpanel" aria-labelledby={`${prefix}-discussion-tab`} hidden={activeTab !== 'discussion'}>
            <div className="pr-section-heading"><div><h2>{t("pr.discussion")}</h2><span>{threads.length} {t("pr.discussions")} {detail.unresolved_threads.length} {t("pr.unresolved_2")}</span></div><div className="pr-segmented" aria-label={t("pr.discussion_filter")}><button aria-pressed={!unresolvedOnly} onClick={() => setUnresolvedOnly(false)}>{t("pr.all")}</button><button aria-pressed={unresolvedOnly} onClick={() => setUnresolvedOnly(true)}>{t("pr.unresolved")}</button></div></div>
            <div className="pr-discussions">{discussions.length ? discussions.map((thread) => <DiscussionCard key={thread.id} thread={thread} />) : <Empty icon={unresolvedOnly ? <Check /> : <MessageSquareMore />}>{unresolvedOnly ? t("pr.no_unresolved_discussions") : t("pr.no_discussions")}</Empty>}</div>
          </section>
        </main>
        {activeTab === 'overview' ? <aside className="pr-sidebar" aria-label={t("pr.pr_metadata")}>
          <section><h2>{t("pr.author")}</h2>{detail.author ? <Person user={detail.author} /> : <p className="pr-muted">{t("pr.author_information_unavailable")}</p>}</section>
          <section><h2>{t("pr.reviewers_2")} <span>{detail.reviewers.length}</span></h2>{detail.reviewers.length ? detail.reviewers.map((user) => <Person key={user.id} user={user} />) : <p className="pr-muted">{t("pr.no_reviewers_assigned")}</p>}</section>
          <section><h2>{t("inbox.labels")}</h2>{detail.labels?.length ? <div className="pr-labels">{detail.labels.map((label) => <span key={label}>{label}</span>)}</div> : <p className="pr-muted">{t("pr.no_labels")}</p>}</section>
          {detail.milestone ? <section><h2>{t("pr.milestone")}</h2><p className="pr-milestone">{detail.milestone}</p></section> : null}
          <section><h2>{t("automations.time")}</h2><dl className="pr-metadata"><div><dt>{t("pr.created")}</dt><dd><time dateTime={detail.created_at}>{formatPrTime(detail.created_at)}</time></dd></div><div><dt>{t("pr.updated")}</dt><dd><time dateTime={detail.updated_at}>{formatPrTime(detail.updated_at)}</time></dd></div></dl></section>
          <section><h2>{t("pr.change_summary")}</h2><dl className="pr-metadata pr-diff-summary"><div><dt><GitCommitHorizontal />{t("inbox.submit")}</dt><dd>{detail.commits_count}</dd></div><div><dt><FileDiff />{t("aow.files")}</dt><dd>{detail.changes_count}</dd></div>{additions != null ? <div><dt>{partialStats ? t("pr.known_added_lines") : t("pr.added_lines")}</dt><dd className="pr-additions">+{additions}</dd></div> : null}{deletions != null ? <div><dt>{partialStats ? t("pr.known_deleted_lines") : t("pr.deleted_lines")}</dt><dd className="pr-deletions">−{deletions}</dd></div> : null}</dl></section>
        </aside> : null}
      </div>
    </div>
  </article>;
}
