import { useRef, useState } from 'react';
import { ArrowDown, ArrowUp, Plus, Trash2 } from 'lucide-react';
import { TaskDialog } from './TaskDialog';
import { tasksApi, taskError } from './api';
import type { TaskBoardData, TaskStatus } from './types';
import { allocateId } from '../../lib/id';

export function StatusDialog({ board, onClose }: { board: TaskBoardData; onClose: () => void }) {
  const revision = useRef(board.status_revision);
  const [statuses, setStatuses] = useState<TaskStatus[]>(board.statuses);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const lock = useRef(false);
  const update = (id: string, patch: Partial<TaskStatus>) => setStatuses(items => items.map(s => s.id === id ? { ...s, ...patch } : s));
  const move = (index: number, delta: number) => setStatuses(items => { const next = [...items]; [next[index], next[index + delta]] = [next[index + delta], next[index]]; return next; });
  const add = async () => {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError('');
    try {
      const id = await allocateId();
      setStatuses(items => [...items, { id, name: '', color: '#7999e8' }]);
    } catch (error) { setError(taskError(error)); }
    finally { lock.current = false; setBusy(false); }
  };
  return <TaskDialog title="配置任务状态" busy={busy} onClose={onClose}><form onSubmit={event => {
    event.preventDefault(); if (lock.current) return; lock.current = true; setBusy(true); setError('');
    void tasksApi.statuses({ expected_revision: revision.current, statuses }).then(onClose).catch(e => setError(taskError(e))).finally(() => { lock.current = false; setBusy(false); });
  }}>
    <fieldset disabled={busy}>
      <p className="tasks-hint">按显示顺序定义状态。任务可以在任意状态间移动；已被任务使用的状态不能删除。</p>
      <div className="tasks-status-editor">{statuses.map((status, index) => <div key={status.id}>
        <input type="color" aria-label={`状态颜色 ${index + 1}`} value={status.color} onChange={e => update(status.id, { color: e.target.value })} />
        <input aria-label={`状态名称 ${index + 1}`} autoFocus={index === 0} required maxLength={128} value={status.name} onChange={e => update(status.id, { name: e.target.value })} />
        <button type="button" aria-label={`上移状态 ${index + 1}`} disabled={index === 0} onClick={() => move(index, -1)}><ArrowUp size={15} /></button>
        <button type="button" aria-label={`下移状态 ${index + 1}`} disabled={index === statuses.length - 1} onClick={() => move(index, 1)}><ArrowDown size={15} /></button>
        <button type="button" aria-label={`删除状态 ${index + 1}`} disabled={statuses.length === 1} onClick={() => setStatuses(items => items.filter(s => s.id !== status.id))}><Trash2 size={15} /></button>
      </div>)}</div>
      <button type="button" onClick={() => void add()}><Plus size={14} />添加状态</button>
    </fieldset>
    {error && <p className="tasks-error" role="alert">{error}</p>}
    <footer><button type="button" disabled={busy} onClick={onClose}>取消</button><button className="tasks-primary" disabled={busy}>保存</button></footer>
  </form></TaskDialog>;
}
