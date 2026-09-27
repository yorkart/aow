import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import { GitPullRequest, LoaderCircle, RefreshCw, UserRound } from 'lucide-react';
import { useReviewTarget } from './reviewProviders';
import { prApi } from './api';
import type { PullRequestSummary, MyPullRequests } from './types';
import { AowIconButton } from '../../components/AowIconButton';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { AowListRow } from '../../components/AowListRow';

interface Props {
  repository: string;
  visible: boolean;
  activeNumber?: number;
  activeProvider?: string;
  activeRemote?: string;
  onOpen: (pr: PullRequestSummary) => void;
}

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

function relativeTime(value: string) {
  const time = Date.parse(value);
  if (!Number.isFinite(time)) return value;
  const seconds = Math.round((time - Date.now()) / 1000);
  const ranges: [Intl.RelativeTimeFormatUnit, number][] = [
    ['year', 31_536_000], ['month', 2_592_000], ['week', 604_800], ['day', 86_400], ['hour', 3_600], ['minute', 60],
  ];
  const formatter = new Intl.RelativeTimeFormat('zh-CN', { numeric: 'auto' });
  for (const [unit, size] of ranges) if (Math.abs(seconds) >= size) return formatter.format(Math.round(seconds / size), unit);
  return formatter.format(seconds, 'second');
}

function PullRequestRow({ pr, current, selected, onOpen }: {
  pr: PullRequestSummary;
  current: boolean;
  selected: boolean;
  onOpen: () => void;
}) {
  return <AowListRow className={'my-pr-row' + (selected ? ' selected' : '')} icon={<GitPullRequest />}
    title={`${pr.draft ? 'Draft ' : ''}#${pr.number} · ${pr.title}`} menuLabel="Pull Request 操作" onOpen={onOpen}>
    <span className="my-pr-row-details"><small>{pr.source_branch} → {pr.target_branch}</small>
      <span className="my-pr-row-meta">{current ? '当前分支' : relativeTime(pr.updated_at)}</span>
    </span>
  </AowListRow>;
}

export function PullRequestsPanel({ repository, visible, activeNumber, activeProvider, activeRemote, onOpen }: Props) {
  const panelId = useId();
  const selection = useReviewTarget(repository, visible);
  const provider = selection.target?.provider;
  const remote = selection.target?.remote;
  const [collapsed, setCollapsed] = useState<{ user?: boolean; pr?: boolean }>({});
  const [state, setState] = useState<MyPullRequests>();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const requestVersion = useRef(0);

  const load = useCallback(async () => {
    const version = ++requestVersion.current;
    if (!selection.ready) return;
    setLoading(true); setError('');
    try {
      const next = await prApi.myPullRequests(repository, { provider, remote });
      if (version === requestVersion.current) setState(next);
    } catch (reason) {
      if (version === requestVersion.current) { setError(errorMessage(reason)); }
    } finally {
      if (version === requestVersion.current) setLoading(false);
    }
  }, [repository, provider, remote, selection.ready]);

  useEffect(() => {
    setState(undefined); setError(''); setLoading(false);
    if (visible) void load();
    return () => { requestVersion.current += 1; };
  }, [load, visible]);

  const current = useMemo(() => new Set(state?.pull_requests.filter((pr) => pr.source_branch === state.current_branch).map((pr) => pr.number)), [state]);
  const user = state?.current_user;
  const userBodyId = `${panelId}-user`;
  const prBodyId = `${panelId}-pr`;
  return <section className="side-view pull-requests-panel" aria-label="Pull Requests">
    {selection.targets.length > 1 ? <label className="review-remote-picker">Remote<select aria-label="PR remote" value={selection.remote} onChange={e => selection.select(e.target.value)}><option value="">请选择 remote</option>{selection.targets.map(t => <option key={t.remote} value={t.remote}>{t.remote} · {t.provider_name} · {t.repository}</option>)}</select></label> : null}
    {selection.error ? <div className="side-error" role="alert">{selection.error}<button onClick={selection.reload}>重新匹配</button></div> : null}
    <AowPanelStack>
      <AowPanel className="pull-requests-section pull-requests-user-section" title="User" icon={<UserRound />} collapsed={collapsed.user} controlsId={userBodyId}
        onCollapsedChange={user => setCollapsed(value => ({ ...value, user }))} empty={!!state && !user && !error}>
        {user ? <div className="pull-requests-user" title={`${user.display_name || user.username} (@${user.username}) · 当前用户`}>
          <strong>{user.display_name || user.username}</strong>
          <small>@{user.username} · 当前用户</small>
        </div> : <div className="side-empty">{loading ? '正在加载当前用户…' : error ? '未能加载当前用户' : '打开面板后加载当前用户。'}</div>}
      </AowPanel>
      <AowPanel className="pull-requests-section pull-requests-pr-section" bodyClassName="pull-requests-body" empty={!!state && !state.pull_requests.length && !error} title="PR" icon={<GitPullRequest />} collapsed={collapsed.pr} controlsId={prBodyId}
        onCollapsedChange={pr => setCollapsed(value => ({ ...value, pr }))}
        actions={<AowIconButton title="刷新 Pull Requests" aria-label="刷新 Pull Requests" disabled={loading} onClick={selection.reload}><RefreshCw className={loading ? 'spinning' : ''} /></AowIconButton>}>
        {error ? <div className="side-error" role="alert">{error}</div> : null}
        {!state && !loading && !error ? <div className="side-empty">打开面板后加载该用户创建的 Open PR。</div> : null}
        {loading && !state ? <div className="my-pr-state" role="status"><LoaderCircle className="spinning" />正在加载 Pull Requests…</div> : null}
        {state ? <div className="my-pr-list">
          {state.pull_requests.length ? state.pull_requests.map((pr) => <PullRequestRow key={pr.number} pr={pr} current={current.has(pr.number)} selected={activeNumber === pr.number && (!activeProvider || activeProvider === pr.provider) && (!activeRemote || activeRemote === pr.remote)} onOpen={() => onOpen(pr)} />) : <div className="side-empty">当前仓库没有该用户创建的 Open PR。</div>}
        </div> : null}
      </AowPanel>
    </AowPanelStack>
  </section>;
}
