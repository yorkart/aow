import { useRef, useState } from 'react';
import type { AowAgent } from '../agents/types';
import type { AowProject, AowWorktree } from '../../aow/types';
import { TaskDialog } from './TaskDialog';
import { taskError, tasksApi } from './api';
import { useRequestKey } from './useRequestKey';
import type { BoardTask, InboxItem, TaskStatus } from './types';

export function ConvertDialog({ item, statuses, agents, project, worktree, onClose, onCreated }: {
  item: InboxItem; statuses: TaskStatus[]; agents: AowAgent[]; project: AowProject; worktree: AowWorktree;
  onClose: () => void; onCreated: (task: BoardTask) => void;
}) {
  const supported = agents.filter(agent => agent.available && ['codex', 'traecli', 'hermes'].includes(agent.agent_type ?? agent.id));
  const [title, setTitle] = useState(item.title);
  const [description, setDescription] = useState(item.description);
  const [agent, setAgent] = useState(supported[0]?.id ?? '');
  const [status, setStatus] = useState(statuses[0]?.id ?? '');
  const [cwd, setCwd] = useState(worktree.path);
  const [newWorktree, setNewWorktree] = useState(false);
  const [branch, setBranch] = useState('');
  const [baseRef, setBaseRef] = useState(worktree.branch || 'HEAD');
  const [newPath, setNewPath] = useState('');
  const [startNow, setStartNow] = useState(false);
  const [busy, setBusy] = useState(false);
  const lock = useRef(false);
  const [error, setError] = useState('');
  const requestKey = useRequestKey();
  const save = async () => {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError('');
    try {
      const payload = { expected_revision: item.revision, title: title.trim(), description, agent, status_id: status, project_id: project.id, cwd: newWorktree ? newPath : cwd, worktree: newWorktree ? { branch, base_ref: baseRef } : null, start_now: startNow };
      const task = await tasksApi.convert(item, { ...payload, request_key: requestKey.keyFor(payload) });
      onCreated(task); onClose();
    } catch (error) { setError(taskError(error)); }
    finally { lock.current = false; setBusy(false); }
  };
  return <TaskDialog title="转为任务" busy={busy} onClose={onClose}><form onSubmit={event => { event.preventDefault(); void save(); }}>
    <fieldset disabled={busy}>
      <label>任务名称<input autoFocus required maxLength={512} value={title} onChange={e => setTitle(e.target.value)} /></label>
      <label>任务说明<textarea rows={3} maxLength={100000} value={description} onChange={e => setDescription(e.target.value)} /></label>
      <label>Agent<select required value={agent} onChange={e => setAgent(e.target.value)}><option value="" disabled>选择 Agent</option>{supported.map(a => <option key={a.id} value={a.id}>{a.display_name}</option>)}</select></label>
      {!supported.length && <p className="tasks-error">没有可用的交互 Agent。请先在设置中配置 Codex、Trae CLI 或 Hermes。</p>}
      <label className="tasks-check"><input type="checkbox" disabled={project.builtin} checked={newWorktree} onChange={e => setNewWorktree(e.target.checked)} />创建新的 Worktree</label>
      {newWorktree ? <><div className="tasks-form-row"><label>新分支<input placeholder="留空自动生成" value={branch} onChange={e => setBranch(e.target.value)} /></label><label>基于分支 / 引用<input required value={baseRef} onChange={e => setBaseRef(e.target.value)} /></label></div><label>Worktree 路径<input placeholder="留空自动生成" value={newPath} onChange={e => setNewPath(e.target.value)} /></label></> : <label>工作目录<select required value={cwd} onChange={e => setCwd(e.target.value)}>{project.worktrees.map(w => <option key={w.path} value={w.path}>{w.branch || '工作目录'} · {w.path}</option>)}</select></label>}
      <label>初始状态<select required value={status} onChange={e => setStatus(e.target.value)}>{statuses.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
      <label className="tasks-check"><input type="checkbox" checked={startNow} onChange={e => setStartNow(e.target.checked)} />立即执行</label>
      <p className="tasks-hint">{startNow ? '创建 Agent 后立即提交任务内容。' : '创建 Agent 并保留任务，稍后点击「开始执行」提交。'}</p>
    </fieldset>
    {error && <p role="alert" className="tasks-error">{error}</p>}
    <footer><button type="button" disabled={busy} onClick={onClose}>取消</button><button className="tasks-primary" disabled={busy || !agent || !status || !title.trim()}>{busy ? '正在转换…' : '创建任务'}</button></footer>
  </form></TaskDialog>;
}
