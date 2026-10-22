import { t } from '../../lib/i18n';
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { ChevronsUpDown } from 'lucide-react';
import { aowApi } from '../../aow/aowApi';
import type { AowProject, ProjectBranches } from '../../aow/types';
import { workspaceNames, type WorkspaceConfig, type WorkspaceMode } from './types';
import './workspaces.css';

export function WorkspaceSelect({ project, value, disabled = false, allowDynamic = false, onChange, onValidityChange }: {
  project: AowProject;
  value: WorkspaceConfig;
  disabled?: boolean;
  allowDynamic?: boolean;
  onChange: (value: WorkspaceConfig) => void;
  onValidityChange: (ready: boolean) => void;
}) {
  const dynamicGroupName = useId();
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
  const dynamicGroup = value.workspace_mode === 'temporary' || value.workspace_mode === 'dynamic';
  const ready = value.workspace_mode === 'dynamic' ? allowDynamic : value.workspace_mode === 'temporary' || (value.workspace_mode === 'existing'
    ? existingReady : Boolean(branches && value.base_branch && worktrees.length));
  useLayoutEffect(() => { onValidityChange(ready); }, [ready, onValidityChange]);
  const branchOptions = branches ? [...new Set([...branches.branches, ...(value.base_branch ? [value.base_branch] : [])])] : [];

  return <div className="workspace-field">
    <span>{t("automations.workspace_mode")}</span>
    <div className="workspace-modes" role="group" aria-label={t("automations.workspace_mode")}>
      {(['new_worktree', 'existing'] as WorkspaceMode[]).map(mode => <button key={mode} type="button"
        className={value.workspace_mode === mode ? 'selected' : ''} aria-pressed={value.workspace_mode === mode}
        disabled={disabled} onClick={() => onChange({ ...value, workspace_mode: mode })}>{workspaceNames[mode]}</button>)}
      <button type="button" className={dynamicGroup ? 'selected' : ''} aria-pressed={dynamicGroup}
        disabled={disabled} onClick={() => { if (!dynamicGroup) onChange({ ...value, workspace_mode: 'temporary' }); }}>{t("workspaces.dynamic_workspace")}</button>
    </div>
    {dynamicGroup && <div className="workspace-dynamic-options" role="radiogroup" aria-label={t("workspaces.dynamic_workspace_type")}>
      {(['temporary', 'dynamic'] as const).map(mode => <label key={mode}
        title={mode === 'dynamic' && !allowDynamic ? t("workspaces.only_manual_tasks_can_specify_a_workspace_dynamically") : undefined}>
        <input type="radio" name={dynamicGroupName} value={mode} checked={value.workspace_mode === mode}
          disabled={disabled || (mode === 'dynamic' && !allowDynamic)}
          onChange={() => onChange({ ...value, workspace_mode: mode })} />
        <span>{workspaceNames[mode]}</span>
      </label>)}
    </div>}
    {value.workspace_mode === 'new_worktree' ? <>
      <label className="workspace-select">
        <span>{t("workspaces.branch_from")}</span><select aria-label={t("workspaces.branch_from")} required value={branches ? value.base_branch : ''}
          onChange={event => onChange({ ...value, base_branch: event.target.value })} disabled={disabled || !branches || !branchOptions.length}>
          {!branches && <option value="">{error ? t("workspaces.failed_to_load_branches") : t("workspaces.loading_branches")}</option>}
          {branches && !branchOptions.length && <option value="">{t("workspaces.no_branches_available_in_this_project")}</option>}
          {branchOptions.map(branch => <option key={branch} value={branch}>{branch}{branch === branches?.default_branch ? t("workspaces.default") : ''}</option>)}
        </select><ChevronsUpDown size={14} aria-hidden="true" />
      </label>
      {error && <div className="workspace-error"><p role="alert">{error}</p><button type="button" disabled={disabled} onClick={() => setRetry(current => current + 1)}>{t("configuration.reload")}</button></div>}
    </> : value.workspace_mode === 'existing' ? <label className="workspace-select">
      <span>{t("automations.workspace")}</span><select aria-label={t("workspaces.existing_worktree")} required value={existingReady ? value.workspace_path : ''}
        onChange={event => onChange({ ...value, workspace_path: event.target.value })} disabled={disabled || !worktrees.length}>
        {!existingReady && <option value="">{worktrees.length ? t("workspaces.choose_worktree") : t("workspaces.no_worktrees_available_in_this_project")}</option>}
        {worktrees.map(worktree => <option key={worktree.path} value={worktree.path}>{worktree.is_main ? t("instances.main_repository") : ''}{worktree.branch || t("git.detached_head")} · {worktree.path.split(/[\\/]/).filter(Boolean).at(-1)}</option>)}
      </select><ChevronsUpDown size={14} aria-hidden="true" />
    </label> : <p className="workspace-hint">{value.workspace_mode === 'dynamic'
      ? t("workspaces.specify_a_workspace_directory_for_each_run_autopilot_automatically")
      : t("workspaces.create_an_empty_workspace_in_the_system_temporary_directory")}</p>}
    {dynamicGroup && !allowDynamic && <p className="workspace-hint">{t("workspaces.dynamic_workspace_selection_is_only_available_for_manual_tasks")}</p>}
  </div>;
}
