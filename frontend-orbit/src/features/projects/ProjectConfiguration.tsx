import { useEffect, useState } from 'react';
import { FolderOpen } from 'lucide-react';
import { ApiError } from '../../lib/api';
import { useDirty, useOperation } from '../agents/hooks';
import { inspectCodebase, saveProject } from './api';
import { DirectoryPicker } from './DirectoryPicker';
import type { CodebaseInfo, Project, ProjectFields, WorktreeSettings } from './types';

export function ProjectConfiguration({ org, project, onSaved, onCancel, onDirtyChange }: { org: string; project?: Project; onSaved: (project: Project) => void; onCancel: () => void; onDirtyChange: (dirty: boolean) => void }) {
  const initial: ProjectFields = { name: project?.name ?? '', description: project?.description ?? '', codebase_path: project?.codebase_path ?? '', worktree_settings: { base_ref: project?.worktree_settings.base_ref ?? '', branch_template: project?.worktree_settings.branch_template ?? '', parent_dir: project?.worktree_settings.parent_dir ?? '' } };
  const [fields, setFields] = useState(initial); const [picker, setPicker] = useState(false); const [pathError, setPathError] = useState(''); const [info, setInfo] = useState<CodebaseInfo>();
  const [worktreeError, setWorktreeError] = useState<{ field: keyof WorktreeSettings; message: string }>();
  const dirty = JSON.stringify(fields) !== JSON.stringify(initial); useDirty(dirty, onDirtyChange); const { busy, error, perform } = useOperation();
  const path = (value: string) => { setFields(current => ({ ...current, codebase_path: value })); setPathError(''); setInfo(undefined); setWorktreeError(undefined); };
  const inspectedPath = fields.codebase_path || project?.managed_directory;
  useEffect(() => {
    if (!inspectedPath) return;
    const abort = new AbortController();
    const timer = window.setTimeout(() => { void inspectCodebase(org, inspectedPath, abort.signal).then(value => { if (!abort.signal.aborted) { setInfo(value); setPathError(''); } }).catch((error: unknown) => { if (!abort.signal.aborted && fields.codebase_path) setPathError(error instanceof Error ? error.message : '目录不可用。'); }); }, 300);
    return () => { window.clearTimeout(timer); abort.abort(); };
  }, [org, inspectedPath, fields.codebase_path]);
  const setting = (key: keyof WorktreeSettings, value: string) => { setFields(current => ({ ...current, worktree_settings: { ...current.worktree_settings, [key]: value } })); setWorktreeError(undefined); };
  return <><header className="project-panel-heading"><div><h2>{project ? 'Configuration' : '新增项目'}</h2><p className="project-note">配置项目名称、用途和可选的本地 Codebase。</p></div></header><form className="project-form" onSubmit={event => { event.preventDefault(); void perform(async () => {
    setPathError(''); setWorktreeError(undefined);
    if (fields.codebase_path) { try { setInfo(await inspectCodebase(org, fields.codebase_path)); } catch (error) { setPathError(error instanceof Error ? error.message : '目录不可用。'); return; } }
    try { const result = await saveProject(org, fields, project); onDirtyChange(false); onSaved(result); }
    catch (error) {
      if (error instanceof ApiError && ['invalid_codebase', 'codebase_in_use'].includes(error.code ?? '')) setPathError(error.message);
      else if (error instanceof ApiError && ['invalid_base_ref', 'invalid_branch_template', 'invalid_worktree_parent'].includes(error.code ?? '')) setWorktreeError({ field: error.code === 'invalid_base_ref' ? 'base_ref' : error.code === 'invalid_branch_template' ? 'branch_template' : 'parent_dir', message: error.message });
      else throw error;
    }
  }); }}>
    <label>Project name<input autoFocus required maxLength={100} disabled={busy} value={fields.name} onChange={event => setFields({ ...fields, name: event.target.value })} placeholder="例如：aow" /></label>
    <label>Description<textarea maxLength={8000} disabled={busy} value={fields.description} onChange={event => setFields({ ...fields, description: event.target.value })} placeholder="描述项目用途…" /></label>
    <label htmlFor="project-codebase">Codebase path</label><div className="project-path-input"><input id="project-codebase" aria-describedby="project-codebase-note" aria-invalid={Boolean(pathError)} disabled={busy} value={fields.codebase_path} onChange={event => path(event.target.value)} placeholder={project?.managed_directory ?? '留空，由 Orbit 自动分配目录'} /><button type="button" disabled={busy} onClick={() => setPicker(true)}><FolderOpen size={15} />选择目录</button><button type="button" disabled={busy || !fields.codebase_path} onClick={() => path('')}>恢复默认</button></div>
    <p className="project-note" id="project-codebase-note">可替换为已有本地目录的绝对路径。留空使用 Orbit 默认目录，保存后生效；已有任务继续使用已绑定的工作区。</p>
    <div className="project-codebase-directory" aria-label="项目工作目录"><span>{fields.codebase_path ? '自定义本地目录' : 'Orbit 管理的默认目录'}</span><code>{fields.codebase_path || project?.managed_directory || '保存项目后自动分配'}</code><p className="project-note">{fields.codebase_path ? '新任务使用此目录；Git 任务可创建隔离 worktree。' : '普通任务共享此项目目录，用于保存任务文件和执行结果。'}</p></div>
    {pathError && <p className="project-field-error" role="alert">{pathError}</p>}{info && <p className="project-note">{info.is_git ? `Git 仓库 · ${info.branch ?? 'Detached HEAD'}` : '普通目录'} · {info.directory}</p>}{error && <p className="project-error" role="alert">{error}</p>}
    {info?.is_git && <fieldset className="project-worktree-settings" disabled={busy} aria-label="Git worktree 配置"><legend>Git worktree</legend><p className="project-note">用于新任务首次创建独立工作区；已绑定工作区的任务继续复用原目录。</p>
      <label>Base ref<input maxLength={512} value={fields.worktree_settings.base_ref ?? ''} aria-invalid={worktreeError?.field === 'base_ref'} placeholder="HEAD" onChange={event => setting('base_ref', event.target.value)} /></label><p className="project-note">任务分支的起始分支、标签或提交。留空使用 HEAD；远程分支需已在本机获取。</p>{worktreeError?.field === 'base_ref' && <p className="project-field-error" role="alert">{worktreeError.message}</p>}
      <label>Branch template<input maxLength={200} value={fields.worktree_settings.branch_template ?? ''} aria-invalid={worktreeError?.field === 'branch_template'} placeholder={`orbit/${org.slice(0, 8)}/{{issue.identifier}}`} onChange={event => setting('branch_template', event.target.value)} /></label><p className="project-note">支持 {'{{issue.identifier}}'}、{'{{issue.id}}'}、{'{{slug}}'}（任务标题）。留空沿用默认命名。</p>{worktreeError?.field === 'branch_template' && <p className="project-field-error" role="alert">{worktreeError.message}</p>}
      <label>Worktree parent dir<input maxLength={4096} value={fields.worktree_settings.parent_dir ?? ''} aria-invalid={worktreeError?.field === 'parent_dir'} placeholder="例如 ../aow-worktrees" onChange={event => setting('parent_dir', event.target.value)} /></label><p className="project-note">绝对路径或相对仓库根目录的路径，须位于主仓库之外。留空使用仓库同级目录；配置父目录时，按生成的分支名建立子目录。</p>{worktreeError?.field === 'parent_dir' && <p className="project-field-error" role="alert">{worktreeError.message}</p>}
    </fieldset>}
    <footer className="project-actions"><button type="button" disabled={busy} onClick={onCancel}>取消</button><button className="orbit-primary" type="submit" aria-busy={busy} disabled={busy || !fields.name.trim() || Boolean(project && !dirty)}>保存项目</button></footer>
  </form>{picker && <DirectoryPicker org={org} initialPath={fields.codebase_path} onClose={() => setPicker(false)} onSelect={directory => { path(directory); setPicker(false); }} />}</>;
}
