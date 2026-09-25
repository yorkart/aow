import { appLocalStorage } from '../../lib/basePath';
import { Fragment, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import {
  ArrowDown, ArrowUp, Check, ChevronDown, ChevronRight, Cloud, FileDiff, GitBranch, History, List,
  MoreHorizontal, RefreshCw, TreePine,
} from 'lucide-react';
import { gitApi } from './api';
import { useSourceControlPolling } from './useSourceControlPolling';
import { SourceControlChanges, type ChangeItem } from './SourceControlChanges';
import { buildCommitGraph, type CommitGraphRow } from './commitGraph';
import type { GitCommit, GitCommitDetail, GitCommitFile, GitFileStatus, RepositorySummary } from './types';
import { CommitDetailPopover } from './CommitDetailPopover';
import { AowIconButton } from '../../components/AowIconButton';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';

type Layout = 'list' | 'tree';
type CommitState = 'outgoing' | 'pushed';

interface Props {
  root: string;
  visible: boolean;
  refreshGeneration?: number;
  activeDocumentId?: string;
  onOpenDiff: (repo: string, file: GitFileStatus, staged: boolean) => void;
  onOpenCommitDiff: (repo: string, commit: GitCommit, file: GitCommitFile) => void;
  fixedRepository?: RepositorySummary;
  titleCase?: boolean;
  aowHeader?: boolean;
}
interface CommitPopoverState { commit: GitCommit; anchor: HTMLElement; pinned: boolean }
interface CommitDetailCacheEntry { detail?: GitCommitDetail; loading?: boolean; error?: string }
interface SectionHeaderProps {
  title: string;
  icon: ReactNode;
  aow?: boolean;
  hideLegacyHeader?: boolean;
  collapsed?: boolean;
  controlsId?: string;
  count?: number;
  details?: ReactNode;
  actions?: ReactNode;
  menu?: ReactNode;
  onToggle?: () => void;
  children?: ReactNode;
}

function SourceSection({ title, icon, aow = false, hideLegacyHeader, collapsed = false, controlsId, count, details, actions, menu, onToggle, children }: SectionHeaderProps) {
  const trailing = <>{count !== undefined ? <span className="source-section-count">{count}</span> : null}{details}</>;
  if (aow) return <AowPanel
    headerClassName="source-section-header" title={title} icon={icon} collapsed={collapsed}
    controlsId={controlsId} onCollapsedChange={onToggle} details={trailing} actions={actions}
  >{menu}{children}</AowPanel>;
  if (hideLegacyHeader) return <>{children}</>;
  return <><h3 className="source-section-header"><button className="source-section-toggle" type="button" aria-expanded={!collapsed} aria-controls={controlsId} onClick={onToggle}>
    {collapsed ? <ChevronRight /> : <ChevronDown />}{icon}<span className="source-section-title">{title}</span>{trailing}
  </button></h3>{children}</>;
}

const CommitChanges = memo(function CommitChanges({ repository, commit, files, layout, active, onOpen }: {
  repository: string; commit: GitCommit; files: GitCommitFile[]; layout: Layout; active: boolean; onOpen: Props['onOpenCommitDiff'];
}) {
  const items = useMemo<ChangeItem[]>(() => files.map(file => ({
    id: `${commit.id}:${file.path}`, path: file.path, status: file.status, title: `${file.status} · ${file.path}`,
  })), [commit.id, files]);
  const open = useCallback((item: ChangeItem) => {
    const file = files.find(file => file.path === item.path);
    if (file) onOpen(repository, commit, file);
  }, [repository, commit, files, onOpen]);
  return <SourceControlChanges items={items} layout={layout} active={active} onOpen={open} />;
});

const graphLaneGap = 12;
const graphPadding = 7;
const graphRowHeight = 28;
const graphMiddle = graphRowHeight / 2;
const graphNodeRadius = 4.5;

function graphX(lane: number) { return graphPadding + lane * graphLaneGap; }
function graphWidth(row: CommitGraphRow) { return graphX(row.columns - 1) + graphNodeRadius + 1; }

function laneStyle(color: string) { return { '--lane-color': color } as CSSProperties; }

function CommitGraph({ commit, row, expanded }: { commit: GitCommit; row: CommitGraphRow; expanded: boolean }) {
  const state: CommitState = commit.is_pushed ? 'pushed' : 'outgoing';
  const nodeX = graphX(row.lane);
  const width = graphWidth(row);
  return <span className="commit-graph-layer" style={{ width }} aria-hidden="true">
    <svg width={width} height={graphRowHeight} viewBox={`0 0 ${width} ${graphRowHeight}`}>
      {row.edges.map((edge, index) => {
        const from = graphX(edge.from); const to = graphX(edge.to);
        const startY = edge.startsAtNode ? graphMiddle : 0;
        return <path key={`${edge.from}:${edge.to}:${index}`} className="commit-graph-edge" style={laneStyle(edge.color)} d={`M ${from} ${startY} C ${from} ${graphMiddle}, ${to} ${graphMiddle}, ${to} ${graphRowHeight}`} />;
      })}
      {row.incoming.map(line => <path key={line.lane} className="commit-graph-edge" style={laneStyle(line.color)} d={`M ${graphX(line.lane)} 0 C ${graphX(line.lane)} ${graphMiddle / 2}, ${nodeX} ${graphMiddle / 2}, ${nodeX} ${graphMiddle}`} />)}
      <circle className={`commit-graph-node ${state}`} style={laneStyle(row.color)} cx={nodeX} cy={graphMiddle} r={graphNodeRadius} />
    </svg>
    {expanded ? row.continuing.map((line) => <i key={line.lane} className="commit-graph-continuation" style={{ left: graphX(line.lane), ...laneStyle(line.color) }} />) : null}
  </span>;
}

export const SourceControl = memo(function SourceControl({ root, visible, refreshGeneration = 0, activeDocumentId, onOpenDiff, onOpenCommitDiff, fixedRepository, titleCase = false, aowHeader = false }: Props) {
  const [repositories, setRepositories] = useState<RepositorySummary[]>([]);
  const [selectedRepository, setRepository] = useState('');
  const repository = fixedRepository?.path ?? selectedRepository;
  const [workingFiles, setWorkingFiles] = useState<GitFileStatus[]>([]);
  const [commits, setCommits] = useState<GitCommit[]>([]);
  const [expanded, setExpanded] = useState<Record<string, GitCommitFile[] | null>>({});
  const [branchesCollapsedOverride, setBranchesCollapsed] = useState<boolean>();
  const [changesCollapsedOverride, setChangesCollapsed] = useState<boolean>();
  const [commitsCollapsedOverride, setCommitsCollapsed] = useState<boolean>();
  const [layout, setLayout] = useState<Layout>(() => appLocalStorage.getItem('aow-git-layout') === 'tree' ? 'tree' : 'list');
  const [changesLayout, setChangesLayout] = useState<Layout>(() => {
    const saved = appLocalStorage.getItem('aow-git-changes-layout') ?? appLocalStorage.getItem('aow-git-layout');
    return saved === 'tree' ? 'tree' : 'list';
  });
  const [commitsLayout, setCommitsLayout] = useState<Layout>(() => {
    const saved = appLocalStorage.getItem('aow-git-commits-layout') ?? appLocalStorage.getItem('aow-git-layout');
    return saved === 'tree' ? 'tree' : 'list';
  });
  const [layoutMenu, setLayoutMenu] = useState<'all' | 'changes' | 'commits'>();
  const panelRef = useRef<HTMLElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [menuPosition, setMenuPosition] = useState({ x: 0, y: 0 });
  const gitCommandRunning = useRef(false);
  const [gitOperation, setGitOperation] = useState<{ repository: string; command: 'pull' | 'push'; state: 'running' | 'success' | 'error'; message?: string }>();
  const [branch, setBranch] = useState('');
  const [upstream, setUpstream] = useState<string>();
  const [upstreamCommit, setUpstreamCommit] = useState<string>();
  const [ahead, setAhead] = useState(0);
  const [behind, setBehind] = useState(0);
  const [error, setError] = useState('');
  const branchesCollapsed = branchesCollapsedOverride ?? (aowHeader && !repository);
  const changesCollapsed = changesCollapsedOverride ?? (aowHeader && !workingFiles.length && !error);
  const commitsCollapsed = commitsCollapsedOverride ?? (aowHeader && !commits.length && !error);
  const [refreshKey, setRefreshKey] = useState(0);
  const repositoryRef = useRef(repository);
  const detailGenerationRef = useRef(0);
  const detailCacheRef = useRef(new Map<string, CommitDetailCacheEntry>());
  const [detailVersion, setDetailVersion] = useState(0);
  const [commitPopover, setCommitPopover] = useState<CommitPopoverState>();
  const hoverTimerRef = useRef<number | undefined>(undefined);
  const closeTimerRef = useRef<number | undefined>(undefined);
  const suppressFocusPreviewRef = useRef(false);
  repositoryRef.current = repository;

  const clearHoverTimer = () => { if (hoverTimerRef.current !== undefined) { window.clearTimeout(hoverTimerRef.current); hoverTimerRef.current = undefined; } };
  const clearCloseTimer = () => { if (closeTimerRef.current !== undefined) { window.clearTimeout(closeTimerRef.current); closeTimerRef.current = undefined; } };
  const closeCommitPopover = () => { clearHoverTimer(); clearCloseTimer(); setCommitPopover(undefined); };
  const detailKey = (commitId: string) => `${repository}\0${commitId}`;
  const loadCommitDetail = (commit: GitCommit) => {
    const key = detailKey(commit.id);
    const requestedRepository = repository;
    const requestedGeneration = detailGenerationRef.current;
    const cached = detailCacheRef.current.get(key);
    if (cached?.loading || cached?.detail) return;
    detailCacheRef.current.set(key, { loading: true });
    setDetailVersion((value) => value + 1);
    void gitApi.gitCommitDetail(repository, commit.id).then((detail) => {
      if (repositoryRef.current !== requestedRepository || detailGenerationRef.current !== requestedGeneration) return;
      detailCacheRef.current.set(key, { detail });
      setDetailVersion((value) => value + 1);
    }).catch((reason) => {
      if (repositoryRef.current !== requestedRepository || detailGenerationRef.current !== requestedGeneration) return;
      detailCacheRef.current.set(key, { error: reason instanceof Error ? reason.message : String(reason) });
      setDetailVersion((value) => value + 1);
    });
  };
  const showCommitPopover = (commit: GitCommit, anchor: HTMLElement, pinned: boolean) => {
    clearHoverTimer(); clearCloseTimer();
    setCommitPopover({ commit, anchor, pinned });
    loadCommitDetail(commit);
  };
  const scheduleCommitPreview = (commit: GitCommit, anchor: HTMLElement) => {
    clearCloseTimer(); clearHoverTimer();
    if (commitPopover?.pinned) return;
    hoverTimerRef.current = window.setTimeout(() => showCommitPopover(commit, anchor, false), 450);
  };
  const schedulePopoverClose = () => {
    clearHoverTimer(); clearCloseTimer();
    if (commitPopover?.pinned) return;
    closeTimerRef.current = window.setTimeout(() => setCommitPopover(undefined), 160);
  };

  useEffect(() => { appLocalStorage.setItem('aow-git-layout', layout); }, [layout]);
  useEffect(() => { appLocalStorage.setItem('aow-git-changes-layout', changesLayout); }, [changesLayout]);
  useEffect(() => { appLocalStorage.setItem('aow-git-commits-layout', commitsLayout); }, [commitsLayout]);
  useEffect(() => {
    if (!visible) { closeCommitPopover(); setLayoutMenu(undefined); }
  }, [visible]);
  useEffect(() => {
    setLayoutMenu(undefined);
    detailGenerationRef.current += 1;
    detailCacheRef.current.clear();
    closeCommitPopover();
  }, [repository]);
  useLayoutEffect(() => {
    if (!layoutMenu) return;
    const trigger = panelRef.current?.querySelector<HTMLButtonElement>(`[data-source-menu="${layoutMenu}"]`);
    const bounds = menuRef.current?.getBoundingClientRect();
    if (!trigger || !bounds) return;
    const anchor = trigger.getBoundingClientRect();
    setMenuPosition({
      x: Math.max(4, Math.min(anchor.right - bounds.width, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(anchor.bottom, window.innerHeight - bounds.height - 4)),
    });
  }, [layoutMenu]);
  useEffect(() => {
    if (!layoutMenu) return;
    const trigger = panelRef.current?.querySelector<HTMLButtonElement>(`[data-source-menu="${layoutMenu}"]`);
    const dismiss = () => setLayoutMenu(undefined);
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!menuRef.current?.contains(target) && !trigger?.contains(target)) dismiss();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      dismiss();
      trigger?.focus({ preventScroll: true });
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    document.addEventListener('keydown', onKeyDown);
    window.addEventListener('blur', dismiss);
    window.addEventListener('resize', dismiss);
    window.addEventListener('scroll', dismiss, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
      document.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('blur', dismiss);
      window.removeEventListener('resize', dismiss);
      window.removeEventListener('scroll', dismiss, true);
    };
  }, [layoutMenu]);
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || !commitPopover) return;
      const anchor = commitPopover.anchor;
      closeCommitPopover();
      if (anchor.isConnected) {
        suppressFocusPreviewRef.current = true;
        anchor.focus({ preventScroll: true });
        queueMicrotask(() => { suppressFocusPreviewRef.current = false; });
      }
    };
    const onPointerDown = (event: PointerEvent) => {
      if (!commitPopover?.pinned) return;
      const target = event.target as Node;
      if (commitPopover.anchor.contains(target) || (target instanceof Element && target.closest('.commit-detail-popover'))) return;
      closeCommitPopover();
    };
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('pointerdown', onPointerDown);
    return () => { document.removeEventListener('keydown', onKeyDown); document.removeEventListener('pointerdown', onPointerDown); };
  }, [commitPopover]);
  useEffect(() => () => { clearHoverTimer(); clearCloseTimer(); }, []);
  useEffect(() => {
    if (fixedRepository) {
      setRepositories([fixedRepository]);
      setRepository(fixedRepository.path);
      return;
    }
    if (!visible) return;
    const controller = new AbortController();
    void gitApi.repositories(root, controller.signal).then((items) => {
      if (controller.signal.aborted) return;
      setRepositories(items);
      setRepository((current) => items.some((item) => item.path === current)
        ? current
        : items[0]?.path ?? '');
    }).catch((reason: Error) => { if (!controller.signal.aborted) setError(reason.message); });
    return () => controller.abort();
  }, [fixedRepository, root, visible, refreshKey]);
  useEffect(() => {
    setWorkingFiles([]); setBranch(''); setAhead(0); setBehind(0);
    setCommits([]); setUpstream(undefined); setUpstreamCommit(undefined);
    setExpanded({}); setError('');
  }, [repository]);
  const refresh = useSourceControlPolling(repository, visible, refreshGeneration, {
    status: (status) => {
      setWorkingFiles((previous) => previous.length === status.files.length && previous.every((file, index) => {
        const next = status.files[index];
        return file.path === next.path && file.index_status === next.index_status
          && file.worktree_status === next.worktree_status && file.original_path === next.original_path;
      }) ? previous : status.files);
      setBranch(status.branch); setAhead(status.ahead); setBehind(status.behind);
    },
    log: (log) => {
      setCommits((previous) => JSON.stringify(previous) === JSON.stringify(log.commits) ? previous : log.commits);
      setUpstream(log.upstream ?? undefined); setUpstreamCommit(log.upstream_commit ?? undefined);
      const ids = new Set(log.commits.map((commit) => commit.id));
      setExpanded((previous) => Object.keys(previous).every((id) => ids.has(id))
        ? previous : Object.fromEntries(Object.entries(previous).filter(([id]) => ids.has(id))));
      for (const key of detailCacheRef.current.keys()) {
        if (!ids.has(key.slice(key.indexOf('\0') + 1))) detailCacheRef.current.delete(key);
      }
      setCommitPopover((previous) => previous && !ids.has(previous.commit.id) ? undefined : previous);
    },
    error: setError,
  });

  const workingChanges = useMemo<ChangeItem[]>(() => workingFiles.flatMap((file) => {
    const rows: ChangeItem[] = [];
    if (file.index_status !== ' ' && file.index_status !== '?') rows.push({ id: `diff:${repository}:true:${file.path}`, path: file.path, status: `${file.index_status} S`, title: `${file.path} · staged` });
    if (file.worktree_status !== ' ') rows.push({ id: `diff:${repository}:false:${file.path}`, path: file.path, status: file.worktree_status === '?' ? 'U' : file.worktree_status, title: `${file.path} · working tree` });
    return rows;
  }), [workingFiles, repository]);
  const openWorkingChange = useCallback((item: ChangeItem) => {
    const file = workingFiles.find(file => file.path === item.path);
    if (file) onOpenDiff(repository, file, item.id === `diff:${repository}:true:${file.path}`);
  }, [workingFiles, repository, onOpenDiff]);

  const toggleCommit = async (commit: GitCommit) => {
    if (commit.id in expanded) { setExpanded((value) => { const next = { ...value }; delete next[commit.id]; return next; }); return; }
    const requestedRepository = repository;
    const requestedGeneration = detailGenerationRef.current;
    setExpanded((value) => ({ ...value, [commit.id]: null })); setError('');
    try {
      const result = await gitApi.gitCommitFiles(requestedRepository, commit.id);
      if (repositoryRef.current !== requestedRepository || detailGenerationRef.current !== requestedGeneration) return;
      setExpanded((value) => commit.id in value ? { ...value, [commit.id]: result.files } : value);
    } catch (reason) {
      if (repositoryRef.current !== requestedRepository || detailGenerationRef.current !== requestedGeneration) return;
      setExpanded((value) => { const next = { ...value }; delete next[commit.id]; return next; });
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const commitGraph = useMemo(() => buildCommitGraph(commits), [commits]);
  const runGitCommand = async (command: 'pull' | 'push') => {
    setLayoutMenu(undefined);
    if (!repository || gitCommandRunning.current) return;
    const requestedRepository = repository;
    gitCommandRunning.current = true;
    setGitOperation({ repository: requestedRepository, command, state: 'running' });
    try {
      await gitApi.gitSync(requestedRepository, command);
      setGitOperation({ repository: requestedRepository, command, state: 'success' });
    } catch (reason) {
      setGitOperation({ repository: requestedRepository, command, state: 'error', message: reason instanceof Error ? reason.message : String(reason) });
    } finally {
      gitCommandRunning.current = false;
      // A failed pull can still update remote refs or leave conflict files.
      if (repositoryRef.current === requestedRepository) refresh();
    }
  };
  const toggleChangesSection = () => {
    if (!changesCollapsed && layoutMenu === 'changes') setLayoutMenu(undefined);
    setChangesCollapsed(!changesCollapsed);
  };
  const toggleCommitsSection = () => {
    if (!commitsCollapsed) closeCommitPopover();
    if (!commitsCollapsed && layoutMenu === 'commits') setLayoutMenu(undefined);
    setCommitsCollapsed(!commitsCollapsed);
  };
  const sectionIdPrefix = `source-control-${repository || root}`.replace(/[^a-zA-Z0-9_-]/g, '-');
  const branchesSectionId = `${sectionIdPrefix}-branches`;
  const changesSectionId = `${sectionIdPrefix}-changes`;
  const commitsSectionId = `${sectionIdPrefix}-commits`;
  const renderLayoutMenu = (target: 'all' | 'changes' | 'commits') => {
    if (layoutMenu !== target) return null;
    const current = target === 'all' ? layout : target === 'commits' ? commitsLayout : changesLayout;
    const selectLayout = (next: Layout) => {
      if (target === 'all') setLayout(next);
      if (target === 'changes') setChangesLayout(next);
      if (target === 'commits') setCommitsLayout(next);
      setLayoutMenu(undefined);
    };
    return createPortal(<div ref={menuRef} className="project-aow-context-menu layout-menu" role="menu"
      aria-label={`${target === 'all' ? 'Source Control' : target === 'changes' ? 'Changes' : 'Commits'} 视图选项`}
      style={{ left: menuPosition.x, top: menuPosition.y }} onPointerDown={event => event.stopPropagation()} onContextMenu={event => event.preventDefault()}>
      <button role="menuitemradio" aria-checked={current === 'list'} onClick={() => selectLayout('list')}><List /> View as list{current === 'list' ? <Check className="layout-menu-check" /> : null}</button>
      <button role="menuitemradio" aria-checked={current === 'tree'} onClick={() => selectLayout('tree')}><TreePine /> View as Tree{current === 'tree' ? <Check className="layout-menu-check" /> : null}</button>
      {target === 'commits' ? <>
        <hr />
        <button role="menuitem" disabled={!repository || gitOperation?.state === 'running'} onClick={() => void runGitCommand('pull')}><ArrowDown /> git pull</button>
        <button role="menuitem" disabled={!repository || gitOperation?.state === 'running'} onClick={() => void runGitCommand('push')}><ArrowUp /> git push</button>
      </> : null}
    </div>, document.body);
  };

  const Container = aowHeader ? AowPanelStack : Fragment;
  return (
    <section ref={panelRef} className={`side-view source-control${commitsCollapsed ? ' commits-collapsed' : ''}`}>
      <Container>
      {!aowHeader ? <div className="side-title"><strong>{titleCase ? 'Source Control' : 'SOURCE CONTROL'}</strong><span className="source-actions"><button title="刷新" onClick={() => { setRefreshKey((key) => key + 1); refresh(); }}><RefreshCw size={14} /></button><button title="视图选项" data-source-menu="all" aria-haspopup="menu" aria-expanded={layoutMenu === 'all'} onClick={() => setLayoutMenu((current) => current === 'all' ? undefined : 'all')}><MoreHorizontal size={14} /></button></span>{renderLayoutMenu('all')}</div> : null}
      {!fixedRepository ? <select value={repository} onChange={(event) => setRepository(event.target.value)} aria-label="选择 Git 仓库">
        {repositories.length ? repositories.map((repo) => <option key={repo.path} value={repo.path}>{repo.name} · {repo.branch}</option>) : <option value="">未发现仓库</option>}
      </select> : null}
      <SourceSection hideLegacyHeader aow={aowHeader} title={titleCase ? 'Branches' : 'BRANCHES'} icon={<GitBranch />} collapsed={branchesCollapsed}
        controlsId={branchesSectionId} onToggle={() => setBranchesCollapsed(!branchesCollapsed)}>
      {repository ? <div id={aowHeader ? undefined : branchesSectionId} className="branch-label" hidden={aowHeader && branchesCollapsed}>
        <GitBranch size={13} /><span className="branch-name">{branch}</span>
        {upstream ? <span className="branch-upstream" title={`Tracking ${upstream}${ahead ? ` · ahead ${ahead}` : ''}${behind ? ` · behind ${behind}` : ''}`}><Cloud />{upstream}{ahead ? ` ↑${ahead}` : ''}{behind ? ` ↓${behind}` : ''}</span> : <span className="branch-upstream missing">未配置 upstream</span>}
      </div> : null}
      </SourceSection>
      {error ? <div className="side-error">{error}</div> : null}
      {gitOperation?.repository === repository ? <div className={gitOperation.state === 'error' ? 'side-error' : 'source-git-status'} role={gitOperation.state === 'error' ? 'alert' : 'status'}>
        git {gitOperation.command}{gitOperation.state === 'running' ? ' 执行中…' : gitOperation.state === 'success' ? ' 已完成' : ` 失败：${gitOperation.message}`}
      </div> : null}
      <SourceSection
        aow={aowHeader} title={titleCase ? 'Changes' : 'CHANGES'} icon={<FileDiff />} collapsed={changesCollapsed}
        controlsId={changesSectionId} count={aowHeader ? undefined : workingChanges.length} onToggle={toggleChangesSection}
        actions={aowHeader ? <>
          <AowIconButton title="刷新 Changes" aria-label="刷新 Changes" onClick={() => refresh('changes')}><RefreshCw /></AowIconButton>
          <AowIconButton title="Changes 视图选项" aria-label="Changes 视图选项" data-source-menu="changes" aria-haspopup="menu" aria-expanded={layoutMenu === 'changes'} onClick={() => setLayoutMenu((current) => current === 'changes' ? undefined : 'changes')}><MoreHorizontal /></AowIconButton>
        </> : undefined}
        menu={aowHeader ? renderLayoutMenu('changes') : undefined}
      >
      <div id={aowHeader ? undefined : changesSectionId} className="changes-list" hidden={changesCollapsed}><SourceControlChanges items={workingChanges} layout={aowHeader ? changesLayout : layout} active={visible && !changesCollapsed} selectedId={activeDocumentId} onOpen={openWorkingChange} />{!workingChanges.length ? <div className="side-empty">没有未提交更改</div> : null}</div>
      </SourceSection>
      <SourceSection
        aow={aowHeader} title={titleCase ? 'Commits' : 'COMMITS'} icon={<History />} collapsed={commitsCollapsed}
        controlsId={commitsSectionId} onToggle={toggleCommitsSection}
        details={aowHeader ? undefined : <span className="commit-legend"><i className="outgoing" />本地<i className="pushed" />已推送</span>}
        actions={aowHeader ? <>
          <AowIconButton title="刷新 Commits" aria-label="刷新 Commits" onClick={() => refresh('commits')}><RefreshCw /></AowIconButton>
          <AowIconButton title="Commits 视图选项" aria-label="Commits 视图选项" data-source-menu="commits" aria-haspopup="menu" aria-expanded={layoutMenu === 'commits'} onClick={() => setLayoutMenu((current) => current === 'commits' ? undefined : 'commits')}><MoreHorizontal /></AowIconButton>
        </> : undefined}
        menu={aowHeader ? renderLayoutMenu('commits') : undefined}
      >
      <div id={aowHeader ? undefined : commitsSectionId} className="commit-list" hidden={commitsCollapsed}>
        {commits.map((commit, index) => {
          const isExpanded = commit.id in expanded; const files = expanded[commit.id];
          const state = commit.is_pushed ? 'pushed' : 'outgoing';
          const graphRow = commitGraph.get(commit.id)!;
          const stateTitle = commit.is_pushed ? `已推送${upstream ? `到 ${upstream}` : ''}` : upstream ? `尚未推送到 ${upstream}` : '仅存在于本地（未配置 upstream）';
          const rowStyle = { '--commit-content-offset': `${graphWidth(graphRow)}px` } as CSSProperties;
          const selected = commitPopover?.commit.id === commit.id;
          const changesId = `${sectionIdPrefix}-commit-${commit.id}`;
          return <div className={`commit-node ${state}${isExpanded ? ' expanded' : ''}${selected ? ' selected' : ''}`} style={rowStyle} key={commit.id} onPointerLeave={schedulePopoverClose}>
            <CommitGraph commit={commit} row={graphRow} expanded={isExpanded} />
            <button
              className="commit-row" type="button" aria-expanded={isExpanded} aria-controls={changesId}
              aria-label={`${isExpanded ? '收起' : '展开'} ${commit.subject} 的文件，${stateTitle}`}
              onPointerEnter={(event) => scheduleCommitPreview(commit, event.currentTarget)}
              onPointerLeave={clearHoverTimer}
              onFocus={(event) => {
                if (suppressFocusPreviewRef.current) { suppressFocusPreviewRef.current = false; return; }
                showCommitPopover(commit, event.currentTarget, false);
              }}
              onBlur={(event) => {
                if (event.relatedTarget instanceof Element && event.relatedTarget.closest('.commit-detail-popover')) return;
                schedulePopoverClose();
              }}
              onClick={() => { closeCommitPopover(); void toggleCommit(commit); }}
            >
              <span className="commit-graph-spacer" aria-hidden="true" />
              <span className="commit-disclosure" aria-hidden="true">{isExpanded ? <ChevronDown /> : <ChevronRight />}</span>
              <span className="commit-main">
                <code>{commit.short_id}</code><span className="commit-subject">{commit.subject}</span>
                <span className="commit-refs">
                  {index === 0 ? <span className="commit-ref local" title={`HEAD · ${branch}`}><GitBranch />{branch}</span> : null}
                  {upstream && commit.id === upstreamCommit ? <span className="commit-ref remote" title={upstream}><Cloud />{upstream}</span> : null}
                </span>
              </span>
            </button>
            {isExpanded ? <div id={changesId} className="commit-changes">{files === null ? <div className="side-empty" role="status">加载中…</div> : files.length ? <CommitChanges repository={repository} commit={commit} files={files} layout={aowHeader ? commitsLayout : layout} active={visible && !commitsCollapsed} onOpen={onOpenCommitDiff} /> : <div className="side-empty">此提交没有文件变更</div>}</div> : null}
          </div>;
        })}
      </div>
      </SourceSection>
      </Container>
      {visible && commitPopover ? (() => {
        const cached = detailCacheRef.current.get(detailKey(commitPopover.commit.id));
        void detailVersion;
        return <CommitDetailPopover
          commit={commitPopover.commit} detail={cached?.detail} loading={cached?.loading === true} error={cached?.error}
          anchor={commitPopover.anchor} pinned={commitPopover.pinned} upstream={upstream}
          onEnter={clearCloseTimer} onLeave={schedulePopoverClose} onClose={closeCommitPopover}
        />;
      })() : null}
    </section>
  );
});
