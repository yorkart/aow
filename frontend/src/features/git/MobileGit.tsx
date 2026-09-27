import { GitBranch } from 'lucide-react';
import { gitApi } from './api';
import { filesApi } from '../files/api';
import { FileTypeIcon } from '../files/FileTypeIcon';
import { MobilePageHeader, MobileRefresh, MobileState } from '../../mobile/MobilePrimitives';
import { useMobileResource, useMobileScroll, type MobileRoute } from '../../mobile/mobileState';
import type { MobileNavigate } from '../../mobile/mobileState';

export function MobileGit({ route, visible, navigate, back }: { route: MobileRoute; visible: boolean; navigate: MobileNavigate; back: () => void }) {
  const status = useMobileResource(`git.${route.workspace}`, () => gitApi.gitStatus(route.workspace), visible && !route.diff);
  const scroll = useMobileScroll(`git.${route.workspace}`, !!status.data);
  if (route.diff) return <MobileDiff route={route} back={back} />;
  return <section className="mobile-content-page">
    <MobilePageHeader title="Git 变更" actions={<MobileRefresh reload={status.reload} loading={status.loading} />} />
    <div className="mobile-scroll" ref={scroll}>
      {status.data && <div className="mobile-branch-card"><GitBranch size={23} /><div><strong>{status.data.branch || 'Detached HEAD'}</strong><span>{status.data.files.length} 个变更 · 领先 {status.data.ahead} / 落后 {status.data.behind}</span></div></div>}
      <MobileState loading={status.loading && !status.data} error={status.error} retry={status.reload} empty={status.data?.files.length === 0 ? '工作区干净，暂无文件变更。' : undefined} />
      <div className="mobile-list">{status.data?.files.map((file) => <div key={file.path} className="mobile-git-row">
        <div className="mobile-git-file"><FileTypeIcon path={file.path} /><span>{file.path}</span></div>
        <div className="mobile-git-actions">
          {file.index_status !== ' ' && file.index_status !== '?' && <button onClick={() => navigate({ workspace: route.workspace, view: 'git', diff: file.path, staged: '1' })}><b>{file.index_status}</b>已暂存 · 查看 Diff</button>}
          {file.worktree_status !== ' ' && <button onClick={() => navigate({ workspace: route.workspace, view: 'git', diff: file.path, untracked: file.worktree_status === '?' ? '1' : undefined })}><b>{file.worktree_status}</b>{file.worktree_status === '?' ? '未跟踪' : '工作区'} · 查看 Diff</button>}
        </div>
      </div>)}</div>
    </div>
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
    <MobilePageHeader title={route.diff!.split('/').pop()!} subtitle={route.commit ? `Commit ${route.commit.slice(0, 8)}` : route.staged === '1' ? '已暂存变更' : '工作区变更'} back={back} actions={<MobileRefresh reload={diff.reload} loading={diff.loading} />} />
    <div className="mobile-scroll mobile-diff-reader" ref={scroll}>
      <MobileState loading={diff.loading && !diff.data} error={diff.error} retry={diff.reload} empty={diff.data && !diff.data.patch ? '暂无文本差异，文件可能是二进制格式。' : undefined} />
      {diff.data?.truncated && <div className="mobile-inline-error">差异较大，仅展示部分内容。</div>}
      <pre className="mobile-diff">{diff.data?.patch.split('\n').map((line, index) => <span key={index} className={line.startsWith('@@') ? 'hunk' : line.startsWith('+') ? 'added' : line.startsWith('-') ? 'removed' : ''}>{line || ' '}<br /></span>)}</pre>
    </div>
  </section>;
}
