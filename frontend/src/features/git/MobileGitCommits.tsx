import { ChevronDown, ChevronRight } from 'lucide-react';
import { t } from '../../lib/i18n';
import { MobileState, mobileTime } from '../../mobile/MobilePrimitives';
import { useMobileResource, useMobileScroll, type MobileNavigate, type MobileRoute } from '../../mobile/mobileState';
import { gitApi } from './api';
import { MobileGitFileRow } from './MobileGitFileRow';

export function MobileGitCommits({ route, visible, refresh, expanded, onExpand, navigate }: {
  route: MobileRoute; visible: boolean; refresh: number; expanded?: string; onExpand: (id?: string) => void; navigate: MobileNavigate;
}) {
  const repository = route.repository ?? route.workspace;
  const log = useMobileResource(`commits.${repository}.${refresh}`, () => gitApi.gitLog(repository), visible);
  const scroll = useMobileScroll(`commits.${repository}`, !!log.data);
  return <div className="mobile-scroll mobile-source-list" ref={scroll}>
    <MobileState loading={log.loading && !log.data} error={log.error} retry={log.reload} empty={log.data?.commits.length === 0 ? t("git.no_commits") : undefined} />
    {log.data?.commits.map(commit => <section key={commit.id} className="mobile-source-commit">
      <button className="mobile-commit-heading" aria-expanded={expanded === commit.id} onClick={() => onExpand(expanded === commit.id ? undefined : commit.id)}>
        {expanded === commit.id ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
        <span className="mobile-commit-text"><strong>{commit.subject}</strong><span><code>{commit.short_id}</code> · {commit.author} · {mobileTime(commit.authored_at)}</span></span>
        {!commit.is_pushed && <span className="mobile-commit-local" title={t("git.local_only")}>{t("git.local")}</span>}
      </button>
      {expanded === commit.id && <MobileCommitFiles repository={repository} commit={commit.id} visible={visible} refresh={refresh}
        onOpen={(path, originalPath) => navigate({ workspace: route.workspace, view: 'git', gitView: 'commits', repository, commit: commit.id, diff: path, originalPath })} />}
    </section>)}
  </div>;
}

function MobileCommitFiles({ repository, commit, visible, refresh, onOpen }: {
  repository: string; commit: string; visible: boolean; refresh: number; onOpen: (path: string, originalPath?: string) => void;
}) {
  const files = useMobileResource(`commit-files.${repository}.${commit}.${refresh}`, () => gitApi.gitCommitFiles(repository, commit), visible);
  return <div className="mobile-commit-files">
    <MobileState loading={files.loading && !files.data} error={files.error} retry={files.reload}
      empty={files.data?.files.length === 0 ? t("git.this_commit_has_no_file_changes") : undefined} />
    {files.data?.files.map(file => <MobileGitFileRow key={file.path} path={file.path} originalPath={file.original_path} status={file.status}
      onOpen={() => onOpen(file.path, file.original_path ?? undefined)} />)}
  </div>;
}
