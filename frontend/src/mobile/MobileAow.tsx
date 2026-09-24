import { appUrl } from '../lib/basePath';
import type { ResolvedTab } from '../aow/tabRoutes';
import { mobileTabTarget } from '../aow/tabRoutes/mobile';
import { lazy, Suspense, useEffect, useState } from 'react';
import { ArrowLeft, ArrowUpRight, CalendarClock, ChevronRight, FolderGit2, FolderOpen, GitBranch, GitPullRequest, Home, MessageSquare, Pin, PinOff, Search, TerminalSquare, X } from 'lucide-react';
import { aowApi } from '../aow/aowApi';
import { usePinnedWorktrees } from '../aow/usePinnedWorktrees';
import { useAowTabLocation } from '../aow/AowTabEntry';
import { WorktreeIcon, worktreeColorValues } from '../aow/WorktreeIcon';
import { AowNodeSwitcher } from '../aow/AowNodeSwitcher';
import type { AowProject, AowWorktree } from '../aow/types';
import { MobileRefresh, MobileState } from './MobilePrimitives';
import { mobileRouteUrl, saveMobileValue, storedMobileValue, useMobileResource, useMobileRoute, useMobileScroll, useMobileViewport, type MobileRoute, type MobileView } from './mobileState';
import type { MobileNavigate } from './mobileState';
import './mobile.css';

const MobileTerminals = lazy(() => import('../features/terminals/MobileTerminals').then((module) => ({ default: module.MobileTerminals })));
const MobileSessions = lazy(() => import('../features/sessions/MobileSessions').then((module) => ({ default: module.MobileSessions })));
const MobileFiles = lazy(() => import('../features/files/MobileFiles').then((module) => ({ default: module.MobileFiles })));
const MobileGit = lazy(() => import('../features/git/MobileGit').then((module) => ({ default: module.MobileGit })));
const MobileAutomations = lazy(() => import('../features/automations/MobileAutomations').then((module) => ({ default: module.MobileAutomations })));
const MobilePullRequests = lazy(() => import('../features/pr/MobilePullRequests').then((module) => ({ default: module.MobilePullRequests })));

const navigation = [
  { id: 'terminal', label: '终端', icon: TerminalSquare },
  { id: 'sessions', label: 'Conversation', icon: MessageSquare },
  { id: 'automations', label: '自动化', icon: CalendarClock },
  { id: 'files', label: '文件', icon: FolderOpen },
  { id: 'git', label: 'Git', icon: GitBranch },
  { id: 'pull-requests', label: 'PR', icon: GitPullRequest },
] as const;

export function MobileAow({ initialEntry }: { initialEntry?: ResolvedTab } = {}) {
  const projects = useMobileResource('projects', () => aowApi.projects());
  const { route, navigate, back } = useMobileRoute(initialEntry, projects.data);
  const syncLocation = useAowTabLocation();
  useEffect(() => {
    if (!projects.data || route.view === 'terminal' && route.workspace) return;
    const target = mobileTabTarget(route, projects.data);
    syncLocation(target, target ? '' : mobileRouteUrl(route));
  }, [route, projects.data, syncLocation]);
  const [headerActions, setHeaderActions] = useState<HTMLDivElement | null>(null);
  const project = projects.data?.find((item) => item.worktrees.some((worktree) => worktree.path === route.workspace));
  const worktree = project?.worktrees.find((item) => item.path === route.workspace);
  useMobileViewport(project?.name ?? '项目');
  const goHome = () => navigate({});
  const fallback = { workspace: route.workspace, view: route.view, ...(route.view === 'files' && route.notes ? { notes: route.notes } : {}) };
  return <div className="mobile-app">
    <header className="mobile-app-header">
      {worktree ? <>
        <button className="mobile-icon-button" aria-label="返回项目列表" onClick={goHome}><ArrowLeft size={21} /></button>
        <button className="mobile-brand-title" onClick={goHome}><strong>{project?.name}</strong><span>{worktree.branch || 'Detached HEAD'}</span></button>
      </> : <MobileNodeSwitcher />}
      <div className="mobile-header-actions" ref={setHeaderActions} />
    </header>
    {route.workspace ? project && worktree ? <MobileWorkspace initialTabId={route.terminal} key={worktree.path} project={project} worktree={worktree} route={route} navigate={navigate} back={() => back(fallback)} headerActions={headerActions} />
      : <div className="mobile-home"><MobileState loading={projects.loading} error={projects.error} retry={projects.reload} empty="工作区不存在或已被移除。" /><button className="mobile-button" onClick={goHome}><Home size={17} />返回项目</button></div>
      : <MobileHome projects={projects.data?.filter(project => !project.builtin) ?? []} loading={projects.loading} error={projects.error} reload={projects.reload} navigate={navigate} />}
  </div>;
}

function MobileNodeSwitcher() {
  const settings = useMobileResource('aow-settings', () => aowApi.settings());
  return <AowNodeSwitcher mobile addresses={settings.data ? settings.data.node_addresses ?? [] : undefined} error={settings.error} />;
}

function MobileHome({ projects, loading, error, reload, navigate }: {
  projects: AowProject[]; loading: boolean; error?: string; reload: () => void; navigate: MobileNavigate;
}) {
  const [query, setQuery] = useState(() => storedMobileValue('project-search'));
  const [pinned, setPinned, pinnedState] = usePinnedWorktrees();
  const scroll = useMobileScroll('projects', !loading);
  const recentPath = storedMobileValue('recent-workspace');
  const recentProject = projects.find((project) => project.worktrees.some((worktree) => worktree.path === recentPath));
  const recent = recentProject?.worktrees.find((worktree) => worktree.path === recentPath);
  const open = (workspace: string) => { saveMobileValue('recent-workspace', workspace); navigate({ workspace, view: 'terminal' }); };
  const filtered = projects.map((project) => ({ ...project, worktrees: project.worktrees.filter((worktree) => `${project.name} ${worktree.branch} ${worktree.path}`.toLowerCase().includes(query.toLowerCase())) })).filter((project) => project.worktrees.length || project.name.toLowerCase().includes(query.toLowerCase()));
  const matchingEntries = new Map(filtered.flatMap((project) => project.worktrees.map((worktree) => [worktree.path, { project, worktree }] as const)));
  const pinnedEntries = [...pinned].flatMap((path) => { const entry = matchingEntries.get(path); return entry ? [entry] : []; });
  const togglePin = (path: string) => setPinned((current) => {
    const next = new Set(current);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    return next;
  });
  return <main className="mobile-home mobile-scroll" ref={scroll}>
    <div className="mobile-home-heading"><div><span className="mobile-eyebrow">WORKSPACE</span><h1>你的项目</h1></div><MobileRefresh reload={() => { reload(); pinnedState.reload(); }} loading={loading || pinnedState.loading} /></div>
    <label className="mobile-search"><Search size={18} /><input type="search" aria-label="搜索项目或分支" placeholder="搜索项目、分支…" value={query} onChange={(event) => { setQuery(event.target.value); saveMobileValue('project-search', event.target.value); }} /></label>
    {(!query || pinnedEntries.length > 0) && <section className="mobile-pinned" aria-label="Pinned">
      <div className="mobile-home-section"><h3 className="mobile-section-label"><Pin size={13} />Pinned</h3><span>{pinnedEntries.length}</span></div>
      {pinnedState.error && <p className="mobile-inline-error" role="alert">{pinnedState.error}</p>}
      {pinnedEntries.length > 0 ? <div className="mobile-project-card">
        {pinnedEntries.map(({ project, worktree }) => <MobileWorktreeRow key={worktree.path} project={project} worktree={worktree} pinned showProject open={open} togglePin={togglePin} />)}
      </div> : <p className="mobile-pinned-empty">{pinnedState.loading ? '正在同步置顶…' : '点击工作区右侧的图钉，置顶常用工作区。'}</p>}
    </section>}
    {!query && recent && <section className="mobile-recent"><h3 className="mobile-section-label">继续上次的工作</h3><button className="mobile-resume-card" onClick={() => open(recent.path)}>
      <div className="mobile-resume-icon"><WorktreeIcon icon={recent.icon} size={24} style={{ color: worktreeColorValues[recent.color ?? 'default'] }} /></div><div className="mobile-row-main"><strong>{recentProject!.name}</strong><span><GitBranch size={13} />{recent.branch || 'Detached HEAD'}</span></div><ArrowUpRight size={20} />
    </button></section>}
    <div className="mobile-home-section"><h3 className="mobile-section-label">全部项目</h3><span>{projects.length}</span></div>
    <MobileState error={error} loading={loading && !projects.length} retry={reload} empty={!loading && !filtered.length ? (query ? '没有匹配的项目或分支。' : '暂无项目，请先在桌面端注册工作区。') : undefined} />
    {filtered.map((project) => <section className="mobile-project-card" key={project.id}>
      <header><div className="mobile-project-icon"><FolderGit2 size={21} /></div><div className="mobile-row-main"><h2>{project.name}</h2><span>{project.worktrees.length} 个工作区</span></div></header>
      {project.error && <p className="mobile-inline-error">{project.error}</p>}
      {project.worktrees.map((worktree) => <MobileWorktreeRow key={worktree.path} project={project} worktree={worktree} pinned={pinned.has(worktree.path)} open={open} togglePin={togglePin} />)}
    </section>)}
    {projects.length > 0 && <p className="mobile-home-footer">{projects.reduce((total, project) => total + project.worktrees.length, 0)} 个工作区 · 随时查看，继续工作</p>}
    <footer className="mobile-home-links">
      <a href={appUrl('/?ui=desktop')}><ArrowUpRight size={15} />打开桌面版</a>
      {window.aowHost?.close && <button onClick={() => window.aowHost?.close?.()}><X size={15} />关闭工作台</button>}
    </footer>
  </main>;
}

function MobileWorktreeRow({ project, worktree, pinned, showProject, open, togglePin }: {
  project: AowProject; worktree: AowWorktree; pinned: boolean; showProject?: boolean; open: (path: string) => void; togglePin: (path: string) => void;
}) {
  const branch = worktree.branch || 'Detached HEAD';
  const label = `${project.name} · ${branch}`;
  return <div className="mobile-worktree-item">
    <button className="mobile-worktree-row" onClick={() => open(worktree.path)}>
      <WorktreeIcon icon={worktree.icon} size={17} style={{ color: worktreeColorValues[worktree.color ?? 'default'] }} /><div className="mobile-row-main"><strong>{branch}</strong><span>{showProject ? `${project.name} · ` : ''}{worktree.path.split('/').filter(Boolean).pop()}</span></div>
      {worktree.is_main && <span className="mobile-badge">主目录</span>}<ChevronRight size={15} />
    </button>
    <button className="mobile-pin-button" aria-label={`${pinned ? '取消置顶' : '置顶'} ${label}`} aria-pressed={pinned} title={pinned ? '取消置顶' : '置顶'} onClick={() => togglePin(worktree.path)}>
      {pinned ? <PinOff size={16} /> : <Pin size={16} />}
    </button>
  </div>;
}

function MobileWorkspace({ project, worktree, route, navigate, back, headerActions, initialTabId }: { initialTabId?: string; project: AowProject; worktree: AowWorktree; route: MobileRoute; navigate: MobileNavigate; back: () => void; headerActions: HTMLElement | null }) {
  const [visited, setVisited] = useState<Set<MobileView>>(new Set([route.view]));
  useEffect(() => {
    setVisited((old) => old.has(route.view) ? old : new Set([...old, route.view]));
    saveMobileValue('recent-workspace', worktree.path);
  }, [route.view, worktree.path]);
  return <main className="mobile-workspace" data-view={route.view}>
    <div className="mobile-workspace-pages">
      {navigation.filter(({ id }) => visited.has(id) || id === route.view).map(({ id }) => <div className="mobile-workspace-page" key={id} hidden={id !== route.view}>
        <Suspense fallback={<MobileState loading />}>
          {id === 'terminal' && <MobileTerminals initialTabId={initialTabId} project={project} worktree={worktree} visible={route.view === id} headerActions={headerActions} />}
          {id === 'sessions' && <MobileSessions route={route} visible={route.view === id} navigate={navigate} back={back} />}
          {id === 'files' && <MobileFiles route={route} notesPath={project.notes_path} visible={route.view === id} navigate={navigate} back={back} />}
          {id === 'git' && <MobileGit route={route} visible={route.view === id} navigate={navigate} back={back} />}
          {id === 'pull-requests' && <MobilePullRequests route={route} visible={route.view === id} navigate={navigate} back={back} />}
          {id === 'automations' && <MobileAutomations route={route} projectId={project.id} visible={route.view === id} navigate={navigate} back={back} />}
        </Suspense>
      </div>)}
    </div>
    <nav className="mobile-bottom-nav" aria-label="工作区导航">
      {navigation.map(({ id, label, icon: Icon }) => <button key={id} aria-label={label} title={label} aria-current={route.view === id ? 'page' : undefined}
      onClick={() => navigate({ workspace: worktree.path, view: id }, true)}><Icon size={28} /></button>)}</nav>
  </main>;
}
