import { useEffect, useMemo, useRef, useState } from 'react';
import { LoaderCircle, Trash2, X } from 'lucide-react';
import type { AowProject } from './types';
import { aowApi } from './aowApi';
import type { WorktreeRemovalJob, WorktreeRemovalPreview } from './types';
import { removalActive, removalStatusLabel } from './useWorktreeRemovals';

export function WorktreeCleanupDialog({ project, isUnallocated, jobs, progressError, onRefresh, onSubmitted, onClose }: {
  project: AowProject;
  isUnallocated: (path: string) => boolean;
  jobs: WorktreeRemovalJob[];
  progressError: string;
  onRefresh: () => void;
  onSubmitted: (job: WorktreeRemovalJob) => void;
  onClose: () => void;
}) {
  const [onlyUnallocated, setOnlyUnallocated] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [previews, setPreviews] = useState<WorktreeRemovalPreview[]>();
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const controller = useRef<AbortController | undefined>(undefined);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; controller.current?.abort(); };
  }, []);
  const latest = useMemo(() => new Map(jobs.flatMap(job => job.items.map(item => [item.path, item] as const))), [jobs]);
  const visible = project.worktrees.filter(worktree => !onlyUnallocated || isUnallocated(worktree.path));
  const selectable = visible.filter(worktree => !worktree.is_main && !worktree.locked && !removalActiveOrAbsent(worktree.path));
  function removalActiveOrAbsent(path: string) { const item = latest.get(path); return item ? removalActive(item) : false; }
  const selectedPaths = selectable.filter(worktree => selected.has(worktree.path)).map(worktree => worktree.path);
  const dirty = previews?.filter(preview => preview.dirty) ?? [];

  const inspect = async (paths: string[]) => {
    setBusy(true); setError(''); setAcknowledged(false);
    controller.current?.abort();
    const abort = new AbortController();
    controller.current = abort;
    try {
      const result: WorktreeRemovalPreview[] = [];
      // Bound Git scans even for a large selection.
      for (let index = 0; index < paths.length; index += 3) {
        result.push(...await Promise.all(paths.slice(index, index + 3).map(path => aowApi.inspectWorktreeRemoval(project.id, path, abort.signal))));
      }
      if (!abort.signal.aborted) setPreviews(result);
    } catch (reason) {
      if (!abort.signal.aborted) setError(reason instanceof Error ? reason.message : String(reason));
    } finally { if (!abort.signal.aborted) setBusy(false); }
  };
  const submit = async () => {
    if (!previews?.length) return;
    setBusy(true); setError('');
    try {
      const job = await aowApi.removeWorktrees(project.id, previews.map(preview => ({ path: preview.worktree.path, force: preview.dirty })));
      onSubmitted(job);
      if (mounted.current) { setSelected(new Set()); setPreviews(undefined); }
    } catch (reason) {
      if (mounted.current) setError(reason instanceof Error ? reason.message : String(reason));
    } finally { if (mounted.current) setBusy(false); }
  };

  return <div className="project-aow-modal-backdrop" onPointerDown={onClose}>
    <section className="project-aow-modal project-aow-dialog worktree-cleanup-dialog" role="dialog" aria-modal="true" aria-labelledby="worktree-cleanup-title" onPointerDown={event => event.stopPropagation()}>
      <header><div><Trash2 /><strong id="worktree-cleanup-title">批量清理 Worktree</strong><span>{project.name}</span></div><button type="button" title="关闭" aria-label="关闭" onClick={onClose}><X /></button></header>
      <div className="project-aow-dialog-body worktree-cleanup-body">
        <p className="project-aow-form-intro">删除所选 Worktree 的目录和关联 Terminal / Agent，保留 Git 分支。提交后可关闭弹窗，后台会继续清理。</p>
        {previews ? <>
          <strong>确认清理以下 {previews.length} 个 Worktree</strong>
          {previews.map(preview => <div className="worktree-cleanup-preview" key={preview.worktree.path}>
            <code>{preview.worktree.path}</code>
            <small>{preview.terminal_tabs} 个 Terminal · {preview.agent_tabs} 个 Agent{preview.dirty ? ` · ${preview.change_count} 项未提交内容` : ' · 工作区干净'}</small>
            {preview.dirty ? <details><summary>查看将删除的未提交内容</summary><pre>{preview.changes.join('\n')}{preview.truncated ? '\n…更多变更未显示' : ''}</pre></details> : null}
          </div>)}
          {dirty.length ? <label className="worktree-cleanup-check danger"><input type="checkbox" checked={acknowledged} disabled={busy} onChange={event => setAcknowledged(event.target.checked)} />我确认强制删除这 {dirty.length} 个 Worktree 的未提交内容，删除后无法从 AoW 恢复。</label> : null}
        </> : <>
          <div className="worktree-cleanup-filters">
            <label className="worktree-cleanup-check"><input type="checkbox" checked={onlyUnallocated} disabled={busy} onChange={event => { setOnlyUnallocated(event.target.checked); setSelected(new Set()); }} />仅显示未分配资源</label>
            <label className="worktree-cleanup-check"><input type="checkbox" checked={selectable.length > 0 && selectedPaths.length === selectable.length} disabled={busy || !selectable.length} onChange={event => setSelected(new Set(event.target.checked ? selectable.map(worktree => worktree.path) : []))} />全选当前结果</label>
          </div>
          <div className="worktree-cleanup-list">
            {visible.map(worktree => {
              const item = latest.get(worktree.path);
              const unavailable = worktree.is_main || worktree.locked || !!(item && removalActive(item));
              return <label className="worktree-cleanup-row" key={worktree.path}>
                <input type="checkbox" aria-label={`选择 ${worktree.branch || worktree.path}`} disabled={busy || unavailable} checked={!unavailable && selected.has(worktree.path)} onChange={event => setSelected(current => { const next = new Set(current); if (event.target.checked) next.add(worktree.path); else next.delete(worktree.path); return next; })} />
                <span><strong>{worktree.branch || worktree.path.split('/').pop()}</strong><code>{worktree.path}</code></span>
                <small>{worktree.is_main ? '主 Worktree 不可删除' : worktree.locked ? '已锁定' : item && removalActive(item) ? removalStatusLabel[item.status] : isUnallocated(worktree.path) ? '未分配资源' : ''}</small>
              </label>;
            })}
            {!visible.length ? <p className="project-aow-empty">没有符合条件的 Worktree</p> : null}
          </div>
          <small>已选择 {selectedPaths.length} 个 Worktree（每次最多 200 个）</small>
        </>}
        {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
        {progressError ? <div className="project-aow-error" role="alert">{progressError}<button onClick={onRefresh}>刷新进度</button></div> : null}
        {latest.size ? <div className="worktree-cleanup-results" aria-label="清理进度" aria-live="polite">
          <strong>清理进度</strong>
          {[...latest.values()].reverse().map(item => <div className="worktree-cleanup-result" key={item.path}>
            <span>{removalActive(item) ? <LoaderCircle className="spinning" size={13} /> : null}{removalStatusLabel[item.status]}</span><code>{item.path}</code>
            {item.error ? <small className="project-aow-error">{item.error}</small> : null}
            {(item.status === 'failed' || item.status === 'interrupted') && project.worktrees.some(worktree => worktree.path === item.path && !worktree.is_main && !worktree.locked) ? <button disabled={busy || !!previews} onClick={() => void inspect([item.path])}>重新检查并重试</button> : null}
          </div>)}
        </div> : null}
      </div>
      <footer className="project-aow-dialog-footer">
        <button type="button" className="project-aow-dialog-button" disabled={busy && !!previews} onClick={() => { if (previews) { setPreviews(undefined); setError(''); } else onClose(); }}>{previews ? '返回选择' : '关闭'}</button>
        <button type="button" className={`project-aow-dialog-button ${previews ? 'danger' : 'primary'}`} disabled={busy || (previews ? dirty.length > 0 && !acknowledged : !selectedPaths.length || selectedPaths.length > 200)} onClick={() => void (previews ? submit() : inspect(selectedPaths))}>{busy ? previews ? '提交中…' : '检查中…' : previews ? `确认清理 ${previews.length} 个 Worktree` : '检查并确认清理'}</button>
      </footer>
    </section>
  </div>;
}
