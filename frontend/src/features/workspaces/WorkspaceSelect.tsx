import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ChevronsUpDown } from 'lucide-react';
import { aowApi } from '../../aow/aowApi';
import type { AowProject, ProjectBranches } from '../../aow/types';
import { workspaceNames, type WorkspaceConfig, type WorkspaceMode } from './types';
import './workspaces.css';

export function WorkspaceSelect({ project, value, disabled = false, onChange, onValidityChange }: {
  project: AowProject;
  value: WorkspaceConfig;
  disabled?: boolean;
  onChange: (value: WorkspaceConfig) => void;
  onValidityChange: (ready: boolean) => void;
}) {
  const [branches, setBranches] = useState<ProjectBranches>();
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  const current = useRef({ value, onChange });
  current.current = { value, onChange };
  const worktrees = [...project.worktrees].sort((left, right) => Number(right.is_main) - Number(left.is_main));

  useEffect(() => {
    const controller = new AbortController();
    setBranches(undefined);
    setError('');
    void aowApi.projectBranches(project.id, controller.signal).then(result => {
      if (controller.signal.aborted) return;
      setBranches(result);
      const { value: selected, onChange: change } = current.current;
      // Preserve saved references even when they no longer appear in the list.
      if (!selected.base_branch) {
        change({ ...selected, base_branch: result.default_branch });
      }
    }).catch(reason => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
    });
    return () => controller.abort();
  }, [project.id, retry]);

  const existingReady = worktrees.some(worktree => worktree.path === value.workspace_path);
  const ready = value.workspace_mode === 'temporary' || (value.workspace_mode === 'existing'
    ? existingReady : Boolean(branches && value.base_branch && worktrees.length));
  useLayoutEffect(() => { onValidityChange(ready); }, [ready, onValidityChange]);
  const branchOptions = branches ? [...new Set([...branches.branches, ...(value.base_branch ? [value.base_branch] : [])])] : [];

  return <div className="workspace-field">
    <span>工作区方式</span>
    <div className="workspace-modes" role="group" aria-label="工作区方式">
      {(Object.keys(workspaceNames) as WorkspaceMode[]).map(mode => <button key={mode} type="button"
        className={value.workspace_mode === mode ? 'selected' : ''} aria-pressed={value.workspace_mode === mode}
        disabled={disabled} onClick={() => onChange({ ...value, workspace_mode: mode })}>{workspaceNames[mode]}</button>)}
    </div>
    {value.workspace_mode === 'new_worktree' ? <>
      <label className="workspace-select">
        <span>分支来自</span><select aria-label="分支来自" required value={branches ? value.base_branch : ''}
          onChange={event => onChange({ ...value, base_branch: event.target.value })} disabled={disabled || !branches || !branchOptions.length}>
          {!branches && <option value="">{error ? '分支加载失败' : '正在加载分支…'}</option>}
          {branches && !branchOptions.length && <option value="">当前项目没有可选分支</option>}
          {branchOptions.map(branch => <option key={branch} value={branch}>{branch}{branch === branches?.default_branch ? '（默认）' : ''}</option>)}
        </select><ChevronsUpDown size={14} aria-hidden="true" />
      </label>
      {error && <div className="workspace-error"><p role="alert">{error}</p><button type="button" disabled={disabled} onClick={() => setRetry(current => current + 1)}>重新加载</button></div>}
    </> : value.workspace_mode === 'existing' ? <label className="workspace-select">
      <span>工作区</span><select aria-label="已有 Worktree" required value={existingReady ? value.workspace_path : ''}
        onChange={event => onChange({ ...value, workspace_path: event.target.value })} disabled={disabled || !worktrees.length}>
        {!existingReady && <option value="">{worktrees.length ? '请选择 Worktree' : '当前项目没有可选 Worktree'}</option>}
        {worktrees.map(worktree => <option key={worktree.path} value={worktree.path}>{worktree.is_main ? '主仓库 · ' : ''}{worktree.branch || 'detached'} · {worktree.path.split(/[\\/]/).filter(Boolean).at(-1)}</option>)}
      </select><ChevronsUpDown size={14} aria-hidden="true" />
    </label> : <p className="workspace-hint">在系统临时目录中创建空工作区。</p>}
  </div>;
}
