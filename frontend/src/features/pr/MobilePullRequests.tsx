import { t } from '../../lib/i18n';
import { useReviewTarget } from './reviewProviders';
import { lazy, Suspense, useState } from 'react';
import { ArrowRight, ChevronRight, GitBranch, GitPullRequest, Search } from 'lucide-react';
import { prApi } from './api';
import { formatPrTime, parsePullRequestNumber } from './pullRequestPresentation';
import { MobilePageHeader, MobileRefresh, MobileState } from '../../mobile/MobilePrimitives';
import { saveMobileValue, storedMobileValue, useMobileResource, useMobileScroll, type MobileRoute } from '../../mobile/mobileState';
import type { MobileNavigate } from '../../mobile/mobileState';
import './MobilePullRequests.css';

const PullRequestDetailView = lazy(() => import('./PullRequestDetailView').then((module) => ({ default: module.PullRequestDetailView })));

export function MobilePullRequests({ route, visible, navigate, back }: {
  route: MobileRoute; visible: boolean; navigate: MobileNavigate; back: () => void;
}) {
  const selection = useReviewTarget(route.workspace, visible && !route.pr);
  const searchKey = `pull-requests.search.${route.workspace}`;
  const [query, setQuery] = useState(() => storedMobileValue(searchKey));
  const list = useMobileResource(`pull-requests.${route.workspace}.${selection.target?.provider}.${selection.remote}`, () => prApi.myPullRequests(route.workspace, selection.target), visible && !route.pr && selection.ready);
  const scroll = useMobileScroll(`pull-requests.${route.workspace}`, !!list.data && !route.pr);
  const number = parsePullRequestNumber(route.pr);

  if (route.pr) return <section className="mobile-content-page mobile-pr-reader" aria-label={t("pr.pull_request_details")}>
    {number ? <Suspense fallback={<><MobilePageHeader title={`PR #${number}`} back={back} /><MobileState loading /></>}>
      <PullRequestDetailView repository={route.repository ?? route.workspace} number={number} provider={route.provider} remote={route.remote} visible={visible} mobile onBack={back} />
    </Suspense> : <><MobilePageHeader title="Pull Request" back={back} /><MobileState empty={t("pr.invalid_pr_number_return_to_the_list_and_choose")} /></>}
  </section>;

  const term = query.trim().toLowerCase();
  const requests = list.data?.pull_requests.filter((pr) => `${pr.title} #${pr.number} ${pr.source_branch} ${pr.target_branch}`.toLowerCase().includes(term)) ?? [];
  return <section className="mobile-content-page mobile-pr-list-page" aria-label={t("navigation.my_pull_requests")}>
    <MobilePageHeader title={t("pr.my_pull_requests")} subtitle={t("pr.open_prs_created_by_the_current_account")} actions={<MobileRefresh reload={selection.reload} loading={list.loading} />} />
    {selection.targets.length > 1 ? <label className="review-remote-picker">{t("navigation.remote")}<select aria-label={t("navigation.pr_remote")} value={selection.remote} onChange={e => selection.select(e.target.value)}><option value="">{t("pr.choose_a_remote")}</option>{selection.targets.map(t => <option key={t.remote} value={t.remote}>{t.remote} · {t.provider_name} · {t.repository}</option>)}</select></label> : null}
    {selection.error ? <div className="mobile-inline-error" role="alert">{selection.error}<button onClick={selection.reload}>{t("pr.match_again")}</button></div> : null}
    <label className="mobile-search mobile-pr-search"><Search size={18} /><input type="search" aria-label={t("pr.search_prs")} placeholder={t("pr.search_title_number_or_branch")} value={query} onChange={(event) => { setQuery(event.target.value); saveMobileValue(searchKey, event.target.value); }} /></label>
    <div className="mobile-scroll" ref={scroll}>
      {list.data ? <div className="mobile-pr-list-meta"><span>{list.data.current_user.display_name || list.data.current_user.username} · {requests.length} / {list.data.pull_requests.length}</span></div> : null}
      {list.error && list.data ? <div className="mobile-inline-error" role="alert">{t("pr.refresh_failed_previous_list_retained")}{list.error}</div> : null}
      <MobileState error={!list.data ? list.error : undefined} loading={list.loading && !list.data} retry={list.reload}
        empty={list.data && !requests.length ? term ? t("pr.no_matching_prs_try_another_title_or_branch") : t("pr.no_open_prs_created_by_you_in_this_repository") : undefined} />
      <div className="mobile-pr-cards">{requests.map((pr) => <button className="mobile-pr-card" key={pr.number} onClick={() => navigate({ workspace: route.workspace, view: 'pull-requests', pr: String(pr.number), provider: pr.provider, remote: pr.remote })}>
        <div className="mobile-pr-card-top"><GitPullRequest size={17} /><span>#{pr.number}</span>{pr.draft ? <span className="mobile-badge">{t("pr.draft")}</span> : null}{pr.source_branch === list.data?.current_branch ? <span className="mobile-badge current">{t("pr.current_branch")}</span> : null}<ChevronRight size={16} /></div>
        <strong>{pr.title}</strong>
        <div className="mobile-pr-card-branches"><GitBranch size={13} /><code>{pr.source_branch}</code><ArrowRight size={12} /><code>{pr.target_branch}</code></div>
        <time dateTime={pr.updated_at}>{t("pr.updated")} {formatPrTime(pr.updated_at)}</time>
      </button>)}</div>
    </div>
  </section>;
}
