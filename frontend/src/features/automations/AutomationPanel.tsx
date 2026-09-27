import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { CalendarClock, LoaderCircle, MousePointerClick, Plus, RefreshCw } from 'lucide-react';
import type { AowAgent } from '../agents/types';
import type { AowProject } from '../../aow/types';
import { withFloatingOpen } from '../../aow/floatingWorkspaceState';
import { AutomationEditor } from './AutomationEditor';
import { AutomationTaskBadge } from './AutomationTaskBadge';
import { AutomationTaskMenu } from './AutomationTaskMenu';
import { deleteTaskConfirmation } from './deleteTaskConfirmation';
import { variableNames } from './variables';
import { automationApi } from './api';
import { errorMessage, scheduleName } from './presentation';
import type { AutomationTask, SchedulerStatus, TaskKind } from './types';
import { useConfirmation } from '../../components/ConfirmationDialog';
import { AowIconButton } from '../../components/AowIconButton';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { AowListRow } from '../../components/AowListRow';
import './automations.css';

interface AutomationPanelProps {
  project: AowProject;
  agents: AowAgent[];
  activeTaskId?: string;
  refreshKey?: number;
  onOpenTask: (task: AutomationTask) => void;
  onTaskChanged: (task?: AutomationTask, mutated?: boolean) => void;
  onTaskDeleted: (taskId: string) => void;
}

export function AutomationPanel({ project, agents, activeTaskId, refreshKey = 0, onOpenTask, onTaskChanged, onTaskDeleted }: AutomationPanelProps) {
  const panelId = useId();
  const [collapsedGroups, setCollapsedGroups] = useState<Partial<Record<TaskKind, boolean>>>({});
  const [tasks, setTasks] = useState<AutomationTask[]>([]);
  const [status, setStatus] = useState<SchedulerStatus>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [editorOpen, setEditorOpen] = useState(false);
  const [creatingKind, setCreatingKind] = useState<TaskKind>('scheduled');
  const [editingTask, setEditingTask] = useState<AutomationTask>();
  const [busy, setBusy] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ taskId: string; x: number; y: number }>();
  const dismissMenu = useCallback(() => setContextMenu(undefined), []);
  const menuTask = contextMenu ? tasks.find(task => task.id === contextMenu.taskId) : undefined;
  const { confirm, confirmationDialog } = useConfirmation();
  const request = useRef(0);

  const refresh = useCallback(async (includeStatus = false) => {
    const current = ++request.current;
    try {
      const [tasksResult, statusResult] = await Promise.allSettled([automationApi.list(project.id), includeStatus ? automationApi.status() : Promise.resolve(undefined)]);
      if (current !== request.current) return;
      if (tasksResult.status === 'fulfilled') { setTasks(tasksResult.value); setError(''); }
      else setError(errorMessage(tasksResult.reason));
      if (statusResult.status === 'fulfilled' && statusResult.value) setStatus(statusResult.value);
    } finally { if (current === request.current) setLoading(false); }
  }, [project.id]);

  useEffect(() => { setLoading(true); void refresh(true); }, [refresh, refreshKey]);

  const apply = (task: AutomationTask) => {
    setTasks((items) => items.some((item) => item.id === task.id)
      ? items.map((item) => item.id === task.id ? task : item)
      : [task, ...items]);
    onTaskChanged(task, true);
  };

  const action = async (task: AutomationTask, kind: 'toggle' | 'delete') => {
    if (busy) return;
    if (kind === 'delete' && !await confirm(deleteTaskConfirmation(task))) return;
    setBusy(true); setError('');
    try {
      if (kind === 'delete') {
        await automationApi.remove(task.id);
        request.current += 1;
        setTasks(items => items.filter(item => item.id !== task.id));
        onTaskDeleted(task.id);
      } else {
        apply(await automationApi.enabled(task.id, !task.enabled));
      }
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(false); }
  };

  return <section className="automation-panel" aria-label={`${project.name} 自动化`}>
    {error ? <div className="automation-panel-error" role="alert">{error}</div> : null}
    <AowPanelStack>
    {(['scheduled', 'manual'] as const).map(kind => {
      const manual = kind === 'manual';
      const items = tasks.filter(task => (task.kind ?? 'scheduled') === kind);
      const label = manual ? '手动任务' : '自动化';
      const title = manual ? 'Manual' : 'Schedule';
      const collapsed = collapsedGroups[kind] ?? (!loading && !error && items.length === 0);
      const bodyId = `${panelId}-${kind}`;
      return <AowPanel className="automation-panel-group" key={kind} bodyClassName="automation-panel-list" bodyRole="list" title={title} icon={manual ? <MousePointerClick /> : <CalendarClock />} collapsed={collapsed} controlsId={bodyId}
          onCollapsedChange={next => { setCollapsedGroups(groups => ({ ...groups, [kind]: next })); dismissMenu(); }} actions={<>
          <AowIconButton title={`刷新${label}`} aria-label={`刷新${label}`} disabled={loading} onClick={() => { setLoading(true); void refresh(true); }}><RefreshCw className={loading ? 'automation-spin' : ''} /></AowIconButton>
          <AowIconButton title={`创建${label}`} aria-label={`创建${label}`} disabled={!!project.error || !project.worktrees.length} onClick={() => { setCreatingKind(kind); setEditingTask(undefined); setEditorOpen(true); }}><Plus /></AowIconButton>
        </>}>
          {loading ? <div className="automation-panel-state"><LoaderCircle className="automation-spin" />正在加载任务…</div> : null}
          {!loading && !items.length ? <div className="automation-panel-state">{manual ? <MousePointerClick /> : <CalendarClock />}<strong>暂无{manual ? '手动任务' : '自动化任务'}</strong><span>点击右上角新增{label}。</span></div> : null}
          {!loading && items.map(task => <AowListRow key={task.id} role="listitem" className={`automation-panel-row${task.id === activeTaskId ? ' active' : ''}`}
            title={task.name} tooltip={`${task.name} · ID: ${task.id}`} icon={manual ? <MousePointerClick /> : <CalendarClock />} menuLabel="任务操作" onOpen={() => onOpenTask(task)}
            menuExpanded={contextMenu?.taskId === task.id} onMenu={(x, y) => setContextMenu({ taskId: task.id, x, y })}>
            <small className="automation-panel-meta"><AutomationTaskBadge task={task} /><span aria-hidden="true">•</span><span className="automation-panel-summary"><code>{task.id}</code><span> · </span>{manual ? `${variableNames(task.prompt_bindings).length} 个变量` : scheduleName(task.cron, task.interval_seconds)}</span></small>
          </AowListRow>)}
      </AowPanel>;
    })}
    </AowPanelStack>
    {contextMenu && menuTask ? <AutomationTaskMenu {...contextMenu} task={menuTask} busy={busy} onClose={dismissMenu}
      onOpen={() => onOpenTask(menuTask)} onOpenFloating={() => withFloatingOpen(() => onOpenTask(menuTask))}
      onEdit={() => { setEditingTask(menuTask); setEditorOpen(true); }}
      onToggle={() => void action(menuTask, 'toggle')} onDelete={() => void action(menuTask, 'delete')} /> : null}
    {editorOpen ? <AutomationEditor task={editingTask} kind={creatingKind} project={project} agents={agents} timezone={status?.timezone} onClose={() => setEditorOpen(false)} onSaved={(task) => { setEditorOpen(false); apply(task); if (!editingTask) onOpenTask(task); }} /> : null}
    {confirmationDialog}
  </section>;
}
