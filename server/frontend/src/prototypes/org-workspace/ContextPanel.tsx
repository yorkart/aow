import { useState } from 'react';
import { ChevronDown, ChevronRight, FileCode2, FileText, Files, Folder, GitBranch, GitPullRequest, PanelRightClose } from 'lucide-react';
import { EmptyState, Modal } from './ui';
import type { Project, Task } from './model';
import { fixturePaths, repositoryFixture } from './workspaceModel';

export type ContextView = 'explorer' | 'source' | 'prs';

export function ContextPanel({ task, project, view, onView, onClose, onOpenFile }: { task: Task; project?: Project; view: ContextView; onView: (view: ContextView) => void; onClose: () => void; onOpenFile: (path: string) => void }) {
  const [expanded, setExpanded] = useState(true);
  const [file, setFile] = useState('');
  const repository = Boolean(project?.repository);
  const hasResult = task.progress > 1;
  const fixture = repositoryFixture(task.projectId);
  const changedFiles = fixture.files;
  return <aside className="ow-context" aria-label="任务上下文">
    <header><span>工作上下文</span><button className="ow-icon-button" aria-label="收起上下文" onClick={onClose}><PanelRightClose size={16} /></button></header>
    <div className="ow-context-project"><span className={`ow-project-symbol ow-${project?.color ?? 'mint'}`}><Folder size={17} /></span><span><strong>{project?.name ?? '独立任务'}</strong><small>{repository ? '已关联仓库' : '任务工作目录'}</small></span></div>
    <div className="ow-context-tabs" aria-label="上下文工具">{([{ id: 'explorer', name: 'Explorer', icon: Files }, { id: 'source', name: 'Source Control', icon: GitBranch }, { id: 'prs', name: 'Pull Requests', icon: GitPullRequest }] as const).map(item => <button key={item.id} className={view === item.id ? 'active' : ''} aria-label={item.name} aria-pressed={view === item.id} title={item.name} onClick={() => onView(item.id)}><item.icon size={16} />{view === item.id && <span>{item.name}</span>}</button>)}</div>
    <div className="ow-context-body">
      {view === 'explorer' && <><button className="ow-tree-root" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />}<Folder size={14} />{project?.name ?? task.id}<span>{task.worktree ? 'worktree' : ''}</span></button>{expanded && <div className="ow-file-tree">{repository ? <><div><ChevronDown size={12} /><Folder size={14} />{fixture.root}</div><div className="ow-indent"><ChevronDown size={12} /><Folder size={14} />src</div><div className="ow-indent-2"><ChevronDown size={12} /><Folder size={14} />{fixture.directory}</div>{changedFiles.map(name => <button className="ow-indent-3" key={name} onClick={() => onOpenFile(`${fixture.root}/src/${fixture.directory}/${name}`)}><FileCode2 size={14} /><span>{name}</span>{hasResult && <em>M</em>}</button>)}{['package.json', 'README.md', '.gitignore'].map(name => <button key={name} onClick={() => onOpenFile(name)}><FileText size={14} />{name}</button>)}</> : <><button onClick={() => onOpenFile('task.md')}><FileText size={14} />task.md</button>{hasResult && <button onClick={() => onOpenFile('result.md')}><FileText size={14} />result.md</button>}</>}</div>}</>}
      {view === 'source' && (repository ? <><div className="ow-branch"><GitBranch size={14} />{task.worktree ? `task/${task.id.toLowerCase()}` : 'main'}</div><div className="ow-context-section-title">Changes <span>{hasResult ? 3 : 0}</span></div>{hasResult ? changedFiles.map((name, index) => <button className="ow-change-row" key={name} onClick={() => onOpenFile(`${fixture.root}/src/${fixture.directory}/${name}`)}><FileCode2 size={14} /><span>{name}</span><small>+{[84, 36, 52][index]}</small><em>M</em></button>) : <p className="ow-context-hint">Agent 正在准备任务，暂时没有文件变更。</p>}{hasResult && <div className="ow-diff-summary"><span>+172</span><span>−24</span><small>演示变更 · 3 个文件</small></div>}</> : <EmptyState icon={<GitBranch />} title="没有关联仓库"><p>{project ? '这个项目没有绑定仓库。' : '这项任务没有关联项目。'}<br />任务产物仍可在 Explorer 中查看。</p></EmptyState>)}
      {view === 'prs' && (repository ? <>{hasResult ? <button className="ow-pr-card" onClick={() => setFile('Pull Request #42')}><GitPullRequest size={18} /><span><strong>{task.title}</strong><small>#42 · Draft · 演示数据</small><span className="ow-pr-branch">{task.worktree ? `task/${task.id.toLowerCase()}` : 'main'} → main</span></span></button> : <EmptyState icon={<GitPullRequest />} title="还没有 Pull Request"><p>任务完成后的演示 PR 会显示在这里。</p></EmptyState>}</> : <EmptyState icon={<GitPullRequest />} title="没有关联仓库"><p>关联了仓库的任务可以在这里查看 Pull Requests。</p></EmptyState>)}
      {view === 'explorer' && task.workspace?.documents.filter(item => !fixturePaths(task, repository).includes(item.path)).map(document => <button className="ow-change-row" key={document.path} onClick={() => onOpenFile(document.path)}><FileText size={14} /><span>{document.path}</span><small>{document.content !== document.saved ? '●' : ''}</small></button>)}
    </div>
    <footer className="ow-context-footer"><span><Folder size={13} />当前 task 的工作目录</span><code>{task.cwd}</code><div><span className="ow-environment-dot" />{task.worktree ? '独立 worktree' : repository ? '项目仓库' : '自动分配的目录'}<span>{task.id}</span></div></footer>
    {file && <Modal title={file} onClose={() => setFile('')}><div className="ow-file-preview"><span className="ow-small-label">只读演示内容</span><p>{task.title}</p><p>该演示 PR 展示当前任务的改动汇总。</p><div className="ow-diff-summary"><span>+172</span><span>−24</span></div></div></Modal>}
  </aside>;
}
