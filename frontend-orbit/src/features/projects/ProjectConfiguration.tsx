import { useState } from 'react';
import { FolderOpen } from 'lucide-react';
import { ApiError } from '../../lib/api';
import { useDirty, useOperation } from '../agents/hooks';
import { inspectCodebase, saveProject } from './api';
import { DirectoryPicker } from './DirectoryPicker';
import type { CodebaseInfo, Project, ProjectFields } from './types';

export function ProjectConfiguration({ org, project, onSaved, onCancel, onDirtyChange }: { org: string; project?: Project; onSaved: (project: Project) => void; onCancel: () => void; onDirtyChange: (dirty: boolean) => void }) {
  const initial: ProjectFields = { name: project?.name ?? '', description: project?.description ?? '', codebase_path: project?.codebase_path ?? '' };
  const [fields, setFields] = useState(initial); const [picker, setPicker] = useState(false); const [pathError, setPathError] = useState(''); const [info, setInfo] = useState<CodebaseInfo>();
  const dirty = JSON.stringify(fields) !== JSON.stringify(initial); useDirty(dirty, onDirtyChange); const { busy, error, perform } = useOperation();
  const path = (value: string) => { setFields(current => ({ ...current, codebase_path: value })); setPathError(''); setInfo(undefined); };
  return <><header className="project-panel-heading"><div><h2>{project ? 'Configuration' : '新增项目'}</h2><p className="project-note">配置项目名称、用途和可选的本地 Codebase。</p></div></header><form className="project-form" onSubmit={event => { event.preventDefault(); void perform(async () => {
    setPathError('');
    if (fields.codebase_path) { try { setInfo(await inspectCodebase(org, fields.codebase_path)); } catch (error) { setPathError(error instanceof Error ? error.message : '目录不可用。'); return; } }
    try { const result = await saveProject(org, fields, project); onDirtyChange(false); onSaved(result); }
    catch (error) { if (error instanceof ApiError && ['invalid_codebase', 'codebase_in_use'].includes(error.code ?? '')) setPathError(error.message); else throw error; }
  }); }}>
    <label>Project name<input autoFocus required maxLength={100} disabled={busy} value={fields.name} onChange={event => setFields({ ...fields, name: event.target.value })} placeholder="例如：aow" /></label>
    <label>Description<textarea maxLength={8000} disabled={busy} value={fields.description} onChange={event => setFields({ ...fields, description: event.target.value })} placeholder="描述项目用途…" /></label>
    <label htmlFor="project-codebase">Codebase path</label><div className="project-path-input"><input id="project-codebase" aria-describedby="project-codebase-note" aria-invalid={Boolean(pathError)} disabled={busy} value={fields.codebase_path} onChange={event => path(event.target.value)} placeholder="可选，例如 /workspace/aow" /><button type="button" disabled={busy} onClick={() => setPicker(true)}><FolderOpen size={15} />选择目录</button></div>
    <p className="project-note" id="project-codebase-note">已有本地目录的绝对路径。留空也可创建项目和管理任务；代码执行前需要绑定可用工作区。</p>
    {pathError && <p className="project-field-error" role="alert">{pathError}</p>}{info && <p className="project-note">{info.is_git ? `Git 仓库 · ${info.branch ?? 'Detached HEAD'}` : '普通目录'} · {info.directory}</p>}{error && <p className="project-error" role="alert">{error}</p>}
    <footer className="project-actions"><button type="button" disabled={busy} onClick={onCancel}>取消</button><button className="orbit-primary" type="submit" aria-busy={busy} disabled={busy || !fields.name.trim() || Boolean(project && !dirty)}>保存项目</button></footer>
  </form>{picker && <DirectoryPicker org={org} initialPath={fields.codebase_path} onClose={() => setPicker(false)} onSelect={directory => { path(directory); setPicker(false); }} />}</>;
}
