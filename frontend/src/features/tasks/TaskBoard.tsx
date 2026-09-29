import { useState } from 'react';
import { Archive, ArrowUpRight, Circle, Columns3, Info, Play, RefreshCw, Settings2 } from 'lucide-react';
import { tasksApi, taskError, refreshTasks, type useTaskBoard } from './api';
import type { BoardTask } from './types';
import { StatusDialog } from './StatusDialog';
import { TaskDialog } from './TaskDialog';

const phases = { preparing: '正在准备 Agent…', ready: '待执行', submitting: '正在提交任务…', submitted: '已提交执行', failed: '执行异常' };
export function TaskBoard({ state, onOpen, selectedId }: {
  state: ReturnType<typeof useTaskBoard>; onOpen: (task: BoardTask) => Promise<void> | void; selectedId?: string;
}) {
  const [query, setQuery] = useState('');
  const [archived, setArchived] = useState(false);
  const [configure, setConfigure] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState<string>();
  const [detailsId, setDetailsId] = useState<string>();
  const [over, setOver] = useState<string>();
  const board = state.board;
  const tasks = board?.tasks.filter(t => t.archived === archived && `${t.title} ${t.description} ${t.cwd}`.toLowerCase().includes(query.toLowerCase())) ?? [];
  const details = board?.tasks.find(t => t.id === detailsId);
  const run = async (id: string, action: () => Promise<unknown> | void) => {
    if (busy) return; setBusy(id); setError('');
    try { await action(); } catch (error) { setError(taskError(error)); } finally { setBusy(undefined); }
  };
  const move = (task: BoardTask, status: string) => { if (task.status_id !== status) void run(task.id, () => tasksApi.move(task, status)); };
  return <section className="tasks-board" aria-label="Task Board">
    <header className="tasks-board-toolbar"><div><Columns3 size={19} /><strong>Task Board</strong><span>{tasks.length}</span></div><div>
      <button aria-label="配置任务状态" title="配置任务状态" disabled={!board} onClick={() => setConfigure(true)}><Settings2 size={16} /></button>
      <button aria-label="刷新任务" title="刷新任务" onClick={() => void refreshTasks()}><RefreshCw size={15} /></button>
    </div></header>
    <div className="tasks-board-filters"><input aria-label="搜索任务" placeholder="搜索任务或工作目录" value={query} onChange={e => setQuery(e.target.value)} /><label className="tasks-check"><input type="checkbox" checked={archived} onChange={e => setArchived(e.target.checked)} />已归档</label><span>从 Inbox 转为任务 · 拖动卡片更新状态</span></div>
    {(error || state.error) && <p role="alert" className="tasks-error">{error || state.error}<button onClick={() => { setError(''); void refreshTasks(); }}>刷新</button></p>}
    {!board && <p className="tasks-empty">正在加载任务…</p>}
    <div className="tasks-columns">{board?.statuses.map(status => {
      const items = tasks.filter(t => t.status_id === status.id);
      return <section className={`tasks-column ${over === status.id ? 'tasks-drop-target' : ''}`} key={status.id} aria-label={status.name} style={{ borderTopColor: status.color }} onDragOver={e => { if (e.dataTransfer.types.includes('application/x-aow-task')) { e.preventDefault(); setOver(status.id); e.dataTransfer.dropEffect = 'move'; } }} onDragLeave={e => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setOver(undefined); }} onDrop={e => {
        e.preventDefault(); setOver(undefined); const task = tasks.find(t => t.id === e.dataTransfer.getData('application/x-aow-task')); if (task) move(task, status.id);
      }}>
        <header><Circle size={14} fill={status.color} color={status.color} /><strong>{status.name}</strong><span>{items.length}</span></header>
        <div className="tasks-column-cards">{!items.length && <div className="tasks-empty-column">暂无任务</div>}{items.map(task => <article key={task.id} className={`tasks-card ${selectedId === task.id ? 'tasks-card-selected' : ''}`} draggable={!busy} onDragStart={e => { e.dataTransfer.setData('application/x-aow-task', task.id); e.dataTransfer.effectAllowed = 'move'; }} onDragEnd={() => setOver(undefined)}>
          <button className="tasks-card-title" onClick={() => task.tab_id ? void run(task.id, () => onOpen(task)) : setDetailsId(task.id)}>{task.title}{task.tab_id && <ArrowUpRight size={15} />}</button>
          {task.description && <p>{task.description}</p>}
          <small title={task.cwd}>{task.agent} · {task.cwd.split('/').filter(Boolean).pop()}</small>
          <div className={`tasks-execution tasks-execution-${task.execution}`}>{phases[task.execution]}</div>
          {task.error && <p className="tasks-card-error" title={task.error}>{task.error}</p>}
          <footer><select aria-label={`任务状态：${task.title}`} value={task.status_id} disabled={!!busy} onChange={e => move(task, e.target.value)}>{board.statuses.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select>
            {task.execution === 'ready' && <button disabled={!!busy} aria-label={`开始执行：${task.title}`} title="开始执行" onClick={() => void run(task.id, () => tasksApi.start(task))}><Play size={14} /></button>}
            <button aria-label={`详情：${task.title}`} title="任务详情" onClick={() => setDetailsId(task.id)}><Info size={14} /></button>
            <button disabled={!!busy} aria-label={`${task.archived ? '恢复' : '归档'}：${task.title}`} title={task.archived ? '恢复任务' : '归档任务'} onClick={() => void run(task.id, () => tasksApi.archive(task))}><Archive size={14} /></button>
          </footer>
        </article>)}</div>
      </section>;
    })}</div>
    {configure && board && <StatusDialog board={board} onClose={() => setConfigure(false)} />}
    {details && <TaskDialog title={details.title} onClose={() => setDetailsId(undefined)}><div className="tasks-detail"><p>{details.description || '暂无补充说明'}</p><dl><dt>任务 ID</dt><dd>{details.id}</dd><dt>工作目录</dt><dd>{details.cwd}</dd><dt>Agent</dt><dd>{details.agent} · {phases[details.execution]}</dd></dl>{details.error && <p role="alert" className="tasks-error">{details.error}</p>}<h3>状态记录</h3><ol>{details.history.slice().reverse().map((change, index) => <li key={index}><strong>{board?.statuses.find(s => s.id === change.status_id)?.name ?? change.status_id}</strong><time>{new Date(change.at).toLocaleString()}</time><p>{change.reason}</p></li>)}</ol></div><footer><button onClick={() => setDetailsId(undefined)}>关闭</button>{details.tab_id && <button className="tasks-primary" onClick={() => { void run(details.id, () => onOpen(details)); setDetailsId(undefined); }}>打开 Agent</button>}</footer></TaskDialog>}
  </section>;
}
