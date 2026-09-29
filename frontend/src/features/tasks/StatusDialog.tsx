import { useRef, useState } from 'react';
import { ArrowDown, ArrowUp, Plus, Trash2 } from 'lucide-react';
import { TaskDialog } from './TaskDialog';
import { tasksApi, taskError } from './api';
import { useRequestKey } from './useRequestKey';
import type { TaskBoardData, TaskStatusWrite } from './types';

export function StatusDialog({ board, onClose }: { board: TaskBoardData; onClose: () => void }) {
  const revision = useRef(board.status_revision);
  const [statuses, setStatuses] = useState<TaskStatusWrite[]>(board.statuses);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const lock = useRef(false);
  const requestKey = useRequestKey();
  const update = (index: number, patch: Partial<TaskStatusWrite>) => setStatuses(items => items.map((s, i) => i === index ? { ...s, ...patch } : s));
  const move = (index: number, delta: number) => setStatuses(items => { const next = [...items]; [next[index], next[index + delta]] = [next[index + delta], next[index]]; return next; });
  return <TaskDialog title="配置任务状态" busy={busy} onClose={onClose}><form onSubmit={event => {
    event.preventDefault(); if (lock.current) return; lock.current = true; setBusy(true); setError('');
    const payload = { expected_revision: revision.current, statuses };
    void tasksApi.statuses({ ...payload, request_key: requestKey.keyFor(payload) }).then(onClose).catch(e => setError(taskError(e))).finally(() => { lock.current = false; setBusy(false); });
  }}>
    <fieldset disabled={busy}>
      <p className="tasks-hint">按显示顺序定义状态。任务可以在任意状态间移动；已被任务使用的状态不能删除。</p>
      <div className="tasks-status-editor">{statuses.map((status, index) => <div key={status.id ?? index}>
        <input type="color" aria-label={`状态颜色 ${index + 1}`} value={status.color} onChange={e => update(index, { color: e.target.value })} />
        <input aria-label={`状态名称 ${index + 1}`} autoFocus={index === 0} required maxLength={128} value={status.name} onChange={e => update(index, { name: e.target.value })} />
        <button type="button" aria-label={`上移状态 ${index + 1}`} disabled={index === 0} onClick={() => move(index, -1)}><ArrowUp size={15} /></button>
        <button type="button" aria-label={`下移状态 ${index + 1}`} disabled={index === statuses.length - 1} onClick={() => move(index, 1)}><ArrowDown size={15} /></button>
        <button type="button" aria-label={`删除状态 ${index + 1}`} disabled={statuses.length === 1} onClick={() => setStatuses(items => items.filter((_, i) => i !== index))}><Trash2 size={15} /></button>
      </div>)}</div>
      <button type="button" onClick={() => setStatuses(items => [...items, { name: '', color: '#7999e8' }])}><Plus size={14} />添加状态</button>
    </fieldset>
    {error && <p className="tasks-error" role="alert">{error}</p>}
    <footer><button type="button" disabled={busy} onClick={onClose}>取消</button><button className="tasks-primary" disabled={busy}>保存</button></footer>
  </form></TaskDialog>;
}
