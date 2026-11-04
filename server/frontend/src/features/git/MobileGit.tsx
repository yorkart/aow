import { t } from '../../lib/i18n';
import { useState } from 'react';
import { ArrowDown, ArrowUp, GitBranch } from 'lucide-react';
import { gitApi } from './api';
import { filesApi } from '../files/api';
import { MobileGitFileRow } from './MobileGitFileRow';
import { MobileGitCommits } from './MobileGitCommits';
import { MobilePageHeader, MobilePanelTabs, MobileRefresh, MobileState } from '../../mobile/MobilePrimitives';
import { useMobileResource, useMobileScroll, type MobileRoute, type MobileNavigate } from '../../mobile/mobileState';
import './mobile-git.css';

const views = [{ id: 'changes', label: t("common.changes") }, { id: 'commits', label: t("common.commits") }] as const;

export function MobileGit({ route, visible, navigate, back }: { route: MobileRoute; visible: boolean; navigate: MobileNavigate; back: () => void }) {
  const [refresh, setRefresh] = useState(0);
  const [expandedCommit, setExpandedCommit] = useState<string>();
  const view = route.gitView === 'commits' || route.commit ? 'commits' : 'changes';
  const repository = route.repository ?? route.workspace;
  const status = useMobileResource(`git.${repository}`, () => gitApi.gitStatus(repository), visible && !route.diff);
  const scroll = useMobileScroll(`git.${repository}.${view}`, !!status.data && !route.diff);
  if (route.diff) return <MobileDiff route={route} back={back} />;
  const staged = status.data?.files.filter(file => file.index_status !== ' ' && file.index_status !== '?') ?? [];
  const working = status.data?.files.filter(file => file.worktree_status !== ' ') ?? [];
  return <section className="mobile-content-page mobile-source-control">
    <MobilePageHeader title={t("navigation.source_control")} actions={<MobileRefresh reload={() => { status.reload(); setRefresh(value => value + 1); }} loading={status.loading} />} />
    <MobilePanelTabs label={t("navigation.source_control")} items={views} selected={view}
      onSelect={gitView => navigate({ workspace: route.workspace, repository: route.repository, view: 'git', gitView }, true)} />
    {status.data && <div className="mobile-source-branch"><GitBranch size={16} /><strong>{status.data.branch || t("git.detached_head")}</strong>
      <span title={t("git.change_summary", { count: status.data.files.length, ahead: status.data.ahead, behind: status.data.behind })}>
        <ArrowUp size={13} />{status.data.ahead}<ArrowDown size={13} />{status.data.behind}
      </span>
    </div>}
    {view === 'commits' ? <MobileGitCommits route={route} visible={visible} refresh={refresh} expanded={expandedCommit} onExpand={setExpandedCommit} navigate={navigate} />
      : <div className="mobile-scroll mobile-source-list" ref={scroll}>
        <MobileState loading={status.loading && !status.data} error={status.error} retry={status.reload}
          empty={status.data?.files.length === 0 ? t("git.clean_worktree_no_changed_files") : undefined} />
        {[{ id: 'staged', label: t("git.staged_changes"), files: staged }, { id: 'working', label: t("git.worktree_changes"), files: working }].map(group => group.files.length > 0 &&
          <section key={group.id} className="mobile-source-group" aria-label={group.label}>
            <header><h3>{group.label}</h3><span>{group.files.length}</span></header>
            {group.files.map(file => <MobileGitFileRow key={file.path} path={file.path} originalPath={file.original_path}
              status={group.id === 'staged' ? file.index_status : file.worktree_status}
              onOpen={() => navigate({ workspace: route.workspace, repository: route.repository, view: 'git', diff: file.path,
                staged: group.id === 'staged' ? '1' : undefined, untracked: group.id === 'working' && file.worktree_status === '?' ? '1' : undefined })} />)}
          </section>)}
      </div>}
  </section>;
}

function MobileDiff({ route, back }: { route: MobileRoute; back: () => void }) {
  const repository = route.repository ?? route.workspace;
  const diff = useMobileResource(`diff.${repository}.${route.diff}.${route.staged}.${route.commit}.${route.originalPath}`, async () => {
    const result = route.commit ? await gitApi.gitCommitDiff(repository, route.commit, route.diff!, route.originalPath)
      : await gitApi.gitDiff(repository, route.diff, route.staged === '1');
    if (route.untracked === '1' && !result.patch && result.modified == null) {
      result.patch = (await filesApi.readText(`${repository}/${route.diff}`)).content.split('\n').map(line => '+' + line).join('\n');
    }
    return result;
  });
  const scroll = useMobileScroll(`diff.${route.workspace}.${route.diff}.${route.staged}`, !!diff.data);
  return <section className="mobile-content-page">
    <MobilePageHeader title={route.diff!.split('/').pop()!} subtitle={route.commit ? `Commit ${route.commit.slice(0, 8)}` : route.staged === '1' ? t("git.staged_changes") : t("git.worktree_changes")} back={back} actions={<MobileRefresh reload={diff.reload} loading={diff.loading} />} />
    <div className="mobile-scroll mobile-diff-reader" ref={scroll}>
      <MobileState loading={diff.loading && !diff.data} error={diff.error} retry={diff.reload} empty={diff.data && !diff.data.patch ? t("git.no_text_diff_this_may_be_a_binary_file") : undefined} />
      {diff.data?.truncated && <div className="mobile-inline-error">{t("git.large_diff_showing_partial_content")}</div>}
      <pre className="mobile-diff">{diff.data?.patch.split('\n').map((line, index) => <span key={index} className={line.startsWith('@@') ? 'hunk' : line.startsWith('+') ? 'added' : line.startsWith('-') ? 'removed' : ''}>{line || ' '}<br /></span>)}</pre>
    </div>
  </section>;
}
