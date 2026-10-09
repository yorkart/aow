import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Bot, LoaderCircle, Search } from 'lucide-react';
import { aowApi } from '../../aow/aowApi';
import type { AowProject } from '../../aow/types';
import { agentsApi } from '../agents/api';
import type { AowAgent } from '../agents/types';
import { AutomationEditor } from '../automations/AutomationEditor';
import { automationApi } from '../automations/api';
import type { AutomationTask } from '../automations/types';
import { terminalApi } from './terminalApi';
import type { TerminalPane, TerminalTab } from './types';
import './terminal-hosting.css';

export function TerminalHostingButton({ tab, pane, onChange }: {
  tab: TerminalTab; pane: TerminalPane; onChange: (tab: TerminalTab) => void;
}) {
  const button = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ left: number; top: number }>();
  const [tasks, setTasks] = useState<AutomationTask[]>([]);
  const [project, setProject] = useState<AowProject>();
  const [creating, setCreating] = useState(false);
  const [editor, setEditor] = useState<{ project: AowProject; agents: AowAgent[] }>();
  const [search, setSearch] = useState('');
  const [maxInputs, setMaxInputs] = useState('3');
  const [runOnEnable, setRunOnEnable] = useState(false);
  const validLimit = Number.isInteger(Number(maxInputs)) && Number(maxInputs) > 0 && Number(maxInputs) <= 4_294_967_295;
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!position) return;
    let active = true;
    setLoading(true); setError(''); setSearch(''); setTasks([]); setProject(undefined); setCreating(false);
    void aowApi.projects().then(projects => {
      const project = projects.find(project => project.worktrees.some(worktree => worktree.path === tab.workspace_root));
      if (!project) throw new Error('当前工作区未关联项目');
      if (active) setProject(project);
      return automationApi.list(project.id);
    }).then(tasks => { if (active) setTasks(tasks.filter(task => task.kind === 'manual' && task.workspace_mode === 'dynamic')); })
      .catch(reason => { if (active) setError(reason instanceof Error ? reason.message : String(reason)); })
      .finally(() => { if (active) setLoading(false); });
    const close = (event: PointerEvent) => {
      if (!popup.current?.contains(event.target as Node) && !button.current?.contains(event.target as Node)) setPosition(undefined);
    };
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') { setPosition(undefined); button.current?.focus(); } };
    const resize = () => setPosition(undefined);
    window.addEventListener('pointerdown', close); window.addEventListener('keydown', key); window.addEventListener('resize', resize);
    return () => { active = false; window.removeEventListener('pointerdown', close); window.removeEventListener('keydown', key); window.removeEventListener('resize', resize); };
  }, [Boolean(position), tab.workspace_root]);

  useEffect(() => {
    if (!position) { setCreating(false); return; }
    if (!creating || !project) return;
    let active = true;
    setError('');
    void agentsApi.agents().then(agents => {
      if (active) { setEditor({ project, agents }); setPosition(undefined); }
    }).catch(reason => { if (active) setError(reason instanceof Error ? reason.message : String(reason)); })
      .finally(() => { if (active) setCreating(false); });
    return () => { active = false; };
  }, [Boolean(position), creating, project]);

  const openMenu = () => {
    const rect = button.current?.getBoundingClientRect();
    if (rect) setPosition({ left: Math.max(8, Math.min(rect.right - 320, window.innerWidth - 328)), top: Math.max(8, Math.min(rect.bottom + 6, window.innerHeight - 360)) });
  };
  const select = async (task: AutomationTask) => {
    if (!validLimit) return;
    setBusy(true); setError('');
    try { onChange(await terminalApi.host(tab.id, pane.id, task.id, task.revision, Number(maxInputs), runOnEnable)); setPosition(undefined); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };
  const matches = tasks.filter(task => `${task.name} ${task.prompt}`.toLowerCase().includes(search.toLowerCase()));
  return <>
    <button ref={button} type="button" className="terminal-hosting-button" title="选择手动任务并开启托管" aria-label="Autopilot"
      aria-haspopup="dialog" aria-expanded={Boolean(position)} disabled={Boolean(pane.hosting) || pane.status !== 'running' || busy || creating}
      onClick={event => {
        event.stopPropagation();
        if (position) setPosition(undefined); else openMenu();
      }}><Bot />Autopilot</button>
    {position && createPortal(<div ref={popup} className="terminal-hosting-menu" style={position} role="dialog" aria-label="选择托管任务"
      onPointerDown={event => event.stopPropagation()} onClick={event => event.stopPropagation()}>
      <strong>选择托管任务</strong>
      <p className="terminal-hosting-limit-hint">仅支持<button type="button" className="terminal-hosting-create" title="创建手动任务" disabled={!project || busy || creating} onClick={() => setCreating(true)}><strong>动态指定工作区</strong></button>的手动任务</p>
      <label className="terminal-hosting-limit">最多自动输入次数<input type="number" min="1" step="1" aria-label="最多自动输入次数" value={maxInputs} disabled={busy} onChange={event => setMaxInputs(event.target.value)} /></label>
      <p className="terminal-hosting-limit-hint">达到上限后仍会做最后一次审查，不再自动输入。</p>
      <label className="terminal-hosting-run-on-enable"><input type="checkbox" checked={runOnEnable} disabled={busy} onChange={event => setRunOnEnable(event.target.checked)} />开启托管后立即执行一次</label>
      <p className="terminal-hosting-limit-hint">不等待当前任务结束；取消勾选则等待下一次完成事件。</p>
      {!validLimit && <p className="terminal-hosting-menu-error" role="alert">请输入有效的正整数</p>}
      <label className="terminal-hosting-search"><Search /><input autoFocus placeholder="搜索手动任务…" aria-label="搜索手动任务" value={search} onChange={event => setSearch(event.target.value)} /></label>
      <div className="terminal-hosting-tasks">
        {loading ? <p role="status"><LoaderCircle className="spinning" />加载中…</p> : matches.map(task => <button key={task.id} type="button" disabled={busy || creating || !validLimit} onClick={() => void select(task)}>
          <span>{task.name}</span><small>{task.prompt.replace(/\s+/g, ' ').slice(0, 100)}</small>
        </button>)}
        {!loading && !matches.length && <p>{tasks.length ? '没有匹配的任务' : '暂无可用任务，点击上方「动态指定工作区」创建手动任务。'}</p>}
      </div>
      {busy && <p role="status">正在开启托管…</p>}
      {creating && <p role="status">正在打开创建弹框…</p>}
      {error && <p className="terminal-hosting-menu-error" role="alert">{error}</p>}
    </div>, document.body)}
    {editor && <AutomationEditor kind="manual" initialWorkspaceMode="dynamic" project={editor.project} agents={editor.agents}
      onClose={() => { setEditor(undefined); requestAnimationFrame(() => button.current?.focus()); }}
      onSaved={() => { setEditor(undefined); openMenu(); }} />}
  </>;
}
