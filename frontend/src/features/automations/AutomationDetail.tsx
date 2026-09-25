import type { AutomationLocation } from '../../aow/tabRoutes';
import { useCallback, useEffect, useRef, useState } from 'react';
import { CalendarClock, Check, ChevronDown, ChevronRight, Clock3, Copy, FileText, History, LoaderCircle, MousePointerClick, Pause, Pencil, Play, RefreshCw, Terminal, Trash2, X, Zap } from 'lucide-react';
import type { AowAgent } from '../agents/types';
import type { AowProject } from '../../aow/types';
import { AgentIcon } from '../agents/AgentIcon';
import { useConfirmation } from '../../components/ConfirmationDialog';
import { MarkdownContent } from '../../components/MarkdownContent';
import { AutomationEditor } from './AutomationEditor';
import { AutomationTaskBadge } from './AutomationTaskBadge';
import { deleteTaskConfirmation } from './deleteTaskConfirmation';
import { ManualRunDialog } from './ManualRunDialog';
import { RunParameters } from './RunParameters';
import { RunParameterSummary } from './RunParameterSummary';
import { variableNames } from './variables';
import { agentNames, dateTime, duration, errorMessage, runNames, scheduleName, workspaceNames } from './presentation';
import { automationApi } from './api';
import type { AutomationRun, AutomationTask, RunOutput, SchedulerStatus } from './types';
import './automations.css';

function RunBadge({ run }: { run: AutomationRun }) {
  return <span className={`automation-badge ${run.status}`}><i />{runNames[run.status]}</span>;
}

function displayedDuration(run: AutomationRun, refreshedAt: number) {
  if (run.status === 'preparing' || run.status === 'running') {
    const startedAt = Date.parse(run.started_at);
    return duration(Number.isNaN(startedAt) ? null : Math.max(0, refreshedAt - startedAt));
  }
  return duration(run.duration_ms);
}

function SessionValue({ value, onOpen }: { value: string; onOpen: () => void }) {
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => { if (!copied) return; const timer = window.setTimeout(() => setCopied(false), 1500); return () => window.clearTimeout(timer); }, [copied]);
  return <span className="automation-copy"><button data-workspace-open className="automation-session-link" title="打开 Agent Session" onClick={onOpen}><code>{value}</code></button><button aria-label="复制会话 ID" title={error || '复制会话 ID'} onClick={async () => { try { await navigator.clipboard.writeText(value); setCopied(true); setError(''); } catch { setError('复制失败，请选中文本复制'); } }}>{copied ? <Check /> : <Copy />}</button>{error ? <small role="status">{error}</small> : null}</span>;
}

function shellQuote(value: string) {
  return /^[a-zA-Z0-9_@%+=:,./-]+$/.test(value) ? value : `'${value.replaceAll("'", "'\"'\"'")}'`;
}

function usePageVisible(visible: boolean) {
  const [pageVisible, setPageVisible] = useState(() => !document.hidden);
  useEffect(() => {
    const update = () => setPageVisible(!document.hidden);
    document.addEventListener('visibilitychange', update);
    update();
    return () => document.removeEventListener('visibilitychange', update);
  }, []);
  return visible && pageVisible;
}

function RunHistory({ taskId, visible, refreshKey, onOpenSession, expanded, onExpandedChange }: { taskId: string; visible: boolean; refreshKey: number; onOpenSession: (run: AutomationRun) => void; expanded?: string; onExpandedChange: (id?: string) => void }) {
  const [runs, setRuns] = useState<AutomationRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [moreBusy, setMoreBusy] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    if (!expanded) return;
    let cancelled = false;
    void automationApi.runDetail(taskId, expanded).then(run => {
      if (!cancelled) setRuns(items => items.some(item => item.id === run.id) ? items : [...items, run]);
    }).catch(reason => { if (!cancelled) setError(errorMessage(reason)); });
    return () => { cancelled = true; };
  }, [taskId, expanded]);
  const [output, setOutput] = useState<{ runId: string; kind: RunOutput; content: string }>();
  const [outputLoading, setOutputLoading] = useState(false);
  const [outputError, setOutputError] = useState('');
  const [refreshedAt, setRefreshedAt] = useState(() => Date.now());
  const loaded = useRef(false);
  const runsRef = useRef(runs);
  runsRef.current = runs;

  useEffect(() => {
    if (!visible) return;
    let active = true;
    let fetching = false;
    const refresh = async () => {
      if (!active || fetching) return;
      fetching = true;
      try {
        const next = await automationApi.runs(taskId);
        if (!active) return;
        const earlierActive = runsRef.current.filter((run) => ['preparing', 'running'].includes(run.status) && !next.some((item) => item.id === run.id));
        const updated = await Promise.all(earlierActive.map((run) => automationApi.runDetail(taskId, run.id)));
        if (!active) return;
        setRuns((current) => {
          const merged = new Map(current.map((run) => [run.id, run]));
          [...next, ...updated].forEach((run) => merged.set(run.id, run));
          return [...merged.values()].sort((a, b) => b.id.localeCompare(a.id));
        });
        setRefreshedAt(Date.now());
        if (!loaded.current) { setHasMore(next.length === 50); loaded.current = true; }
        setError('');
      } catch (reason) { if (active) setError(errorMessage(reason)); }
      finally { fetching = false; if (active) setLoading(false); }
    };
    void refresh();
    const timer = window.setInterval(() => { if (!document.hidden) void refresh(); }, 2500);
    return () => { active = false; window.clearInterval(timer); };
  }, [taskId, visible, refreshKey]);

  const loadMore = async () => {
    setMoreBusy(true);
    try {
      const next = await automationApi.runs(taskId, runs.at(-1)?.id);
      setRuns((current) => [...new Map([...current, ...next].map((run) => [run.id, run])).values()].sort((a, b) => b.id.localeCompare(a.id)));
      setHasMore(next.length === 50); setError('');
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setMoreBusy(false); }
  };

  const loadOutput = async (runId: string, kind: RunOutput) => {
    if (output?.runId === runId && output.kind === kind) {
      setOutput(undefined);
      setOutputError('');
      return;
    }
    setOutputLoading(true);
    setOutputError('');
    try {
      const content = await automationApi.runOutput(taskId, runId, kind);
      setOutput({ runId, kind, content });
    } catch (reason) {
      setOutputError(errorMessage(reason));
    } finally {
      setOutputLoading(false);
    }
  };

  return <div className="automation-history">
    {error ? <div className="automation-error" role="alert">{error}</div> : null}
    {loading ? <div className="automation-empty"><LoaderCircle className="automation-spin" /><span>正在加载执行历史…</span></div>
      : !runs.length ? <div className="automation-empty"><History /><h3>暂无执行记录</h3><p>按计划运行或点击“立即运行”后，执行记录会显示在这里。</p></div>
        : <><div className="automation-run-heading"><span>执行时间</span><span>工作区</span><span>参数</span><span>耗时</span><span>状态</span></div>
          {runs.map((run) => <div className="automation-run" key={run.id}>
            <button className="automation-run-summary" aria-expanded={expanded === run.id} onClick={() => { onExpandedChange(expanded === run.id ? undefined : run.id); setOutput(undefined); setOutputError(''); }}>
              <span>{expanded === run.id ? <ChevronDown /> : <ChevronRight />}<span>{dateTime(run.started_at)}<small>{run.source === 'manual' ? '手动运行' : '计划运行'}</small></span></span>
              <span className="automation-ellipsis" title={run.workspace_path ?? ''}>{run.branch || run.workspace_path?.split('/').pop() || '—'}</span>
              <RunParameterSummary run={run} />
              <span>{displayedDuration(run, refreshedAt)}</span><RunBadge run={run} />
            </button>
            {expanded === run.id ? <div className="automation-run-detail"><dl>
              <div className="automation-full-width"><dt>Session ID</dt><dd>{run.session_id ? <SessionValue value={run.session_id} onOpen={() => onOpenSession(run)} /> : run.status === 'running' || run.status === 'preparing' ? '等待 Agent 会话…' : '未获取'}</dd></div>
              <div><dt>开始时间</dt><dd>{dateTime(run.started_at)}</dd></div><div><dt>结束时间</dt><dd>{dateTime(run.finished_at)}</dd></div>
              <div className="automation-full-width"><dt>工作区</dt><dd><code>{run.workspace_path ?? '—'}</code></dd></div>
              <div className="automation-full-width"><dt>分支</dt><dd><code>{run.branch ?? '—'}</code></dd></div>
              <div><dt>进程 ID</dt><dd>{run.agent_pid ?? '—'}</dd></div><div><dt>退出码</dt><dd>{run.exit_code ?? '—'}</dd></div>
              <div className="automation-full-width"><dt>Agent 命令</dt><dd><code>{run.agent_command?.map(shellQuote).join(' ') ?? '—'}</code></dd></div>
              <div className="automation-full-width"><dt>Run ID</dt><dd><code>{run.id}</code></dd></div>
            </dl><RunParameters run={run} /><div className="automation-run-output"><div className="automation-run-output-heading"><span>执行输出</span><div><button className={output?.runId === run.id && output.kind === 'stdio' ? 'active' : ''} disabled={outputLoading} onClick={() => void loadOutput(run.id, 'stdio')}>stdio</button><button className={output?.runId === run.id && output.kind === 'stderr' ? 'active' : ''} disabled={outputLoading} onClick={() => void loadOutput(run.id, 'stderr')}>stderr</button></div></div>{outputLoading ? <p className="automation-output-state">正在读取输出…</p> : null}{outputError ? <p className="automation-output-state error">{outputError}</p> : null}{output?.runId === run.id ? <pre>{output.content || '(空)'}</pre> : null}</div>{run.message ? <p className={`automation-run-message ${run.status}`}>{run.message}</p> : null}</div> : null}
          </div>)}
          {hasMore ? <button className="automation-load-more" disabled={moreBusy} onClick={() => void loadMore()}>{moreBusy ? '加载中…' : '加载更早的记录'}</button> : null}
        </>}
  </div>;
}

interface AutomationDetailProps {
  initialLocation?: AutomationLocation;
  onLocationChange?: (location: AutomationLocation) => void;
  taskId: string;
  visible: boolean;
  refreshKey?: number;
  project: AowProject;
  agents: AowAgent[];
  onTaskChanged: (task?: AutomationTask, mutated?: boolean) => void;
  onTaskDeleted: (taskId: string) => void;
  onOpenSession: (run: AutomationRun) => void;
}

export function AutomationDetail({ taskId, visible, refreshKey: taskRefreshKey = 0, project, agents, onTaskChanged, onTaskDeleted, onOpenSession, initialLocation, onLocationChange }: AutomationDetailProps) {
  const [task, setTask] = useState<AutomationTask>();
  const [status, setStatus] = useState<SchedulerStatus>();
  const [localLocation, setLocalLocation] = useState<AutomationLocation>({ view: 'overview' });
  const location = initialLocation ?? localLocation;
  const tab = location.view;
  const expandedRun = location.runId;
  const changeLocation = (next: AutomationLocation) => { setLocalLocation(next); onLocationChange?.(next); };
  const setTab = (view: AutomationLocation['view']) => changeLocation({ view, runId: view === 'runs' ? expandedRun : undefined });
  const setExpandedRun = (runId?: string) => changeLocation({ view: 'runs', runId });
  const [editorOpen, setEditorOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [runTask, setRunTask] = useState<AutomationTask>();
  const [refreshKey, setRefreshKey] = useState(0);
  const request = useRef(0);
  const pollingVisible = usePageVisible(visible);
  const { confirm, confirmationDialog } = useConfirmation();

  const refresh = useCallback(async (includeStatus = false) => {
    const current = ++request.current;
    try {
      const [taskResult, statusResult] = await Promise.allSettled([automationApi.detail(taskId), includeStatus ? automationApi.status() : Promise.resolve(undefined)]);
      if (current !== request.current) return;
      if (taskResult.status === 'fulfilled') { setTask(taskResult.value); onTaskChanged(taskResult.value); setError(''); }
      else setError(errorMessage(taskResult.reason));
      if (statusResult.status === 'fulfilled' && statusResult.value) setStatus(statusResult.value);
    } catch (reason) { if (current === request.current) setError(errorMessage(reason)); }
  }, [onTaskChanged, taskId]);

  useEffect(() => {
    if (!pollingVisible) return;
    let active = true;
    let fetching = false;
    const poll = async (includeStatus = false) => {
      if (!active || fetching || document.hidden) return;
      fetching = true;
      try { await refresh(includeStatus); }
      finally { fetching = false; }
    };
    void poll(true);
    const timer = window.setInterval(() => void poll(), 3000);
    return () => {
      active = false;
      window.clearInterval(timer);
      request.current += 1;
    };
  }, [refresh, pollingVisible, taskRefreshKey]);

  const apply = (updated: AutomationTask) => { setTask(updated); onTaskChanged(updated, true); };
  const action = async (kind: 'run' | 'toggle' | 'sync' | 'delete') => {
    if (!task || busy) return;
    if (kind === 'run' && task.kind === 'manual' && task.prompt_bindings?.length) { setRunTask(task); return; }
    if (kind === 'delete' && !await confirm(deleteTaskConfirmation(task))) return;
    setBusy(true); setError(''); setNotice('');
    try {
      if (kind === 'run') {
        const result = await automationApi.run(task.id, task.revision);
        setTab('runs'); setRefreshKey((value) => value + 1);
        setNotice(result.run_id ? '已提交运行，等待 Runner 启动。' : '任务仍在执行，已跳过本次触发。');
      } else if (kind === 'delete') {
        await automationApi.remove(task.id);
        onTaskChanged(); onTaskDeleted(task.id);
        return;
      } else {
        apply(kind === 'toggle' ? await automationApi.enabled(task.id, !task.enabled) : await automationApi.sync(task.id));
      }
      void refresh(true);
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(false); }
  };

  const controls = task ? <div className="automation-actions">
    <button title="立即运行" aria-label="立即运行" className="automation-primary" disabled={busy || (task.max_concurrent_runs === 1 && task.is_running) || status?.ready === false} onClick={() => void action('run')}><Play /><span>立即运行</span></button>
    <button title="编辑" aria-label="编辑" disabled={busy} onClick={() => setEditorOpen(true)}><Pencil /></button>
    {task.kind !== 'manual' ? <button title={task.enabled ? '暂停' : '启用'} aria-label={task.enabled ? '暂停' : '启用'} disabled={busy} onClick={() => void action('toggle')}>{task.enabled ? <Pause /> : <Zap />}</button> : null}
    <button title="删除" aria-label="删除" className="automation-danger" disabled={busy} onClick={() => void action('delete')}><Trash2 /></button>
  </div> : null;

  return <section className="automation-detail-page" aria-label="自动化详情">
    {task ? <header className="automation-detail-heading">
      {task.kind === 'manual' ? <MousePointerClick /> : <CalendarClock />}
      <div className="automation-detail-title">
        <h2 title={task.name}>{task.name}</h2>
        <div><span>{agentNames[task.agent]}</span><span>·</span><span className="automation-detail-workspace" title={`${task.project_name} / ${task.workspace_path}`}>{task.project_name} / {task.workspace_mode === 'temporary' ? '临时目录（TMP）' : task.workspace_path.split('/').pop()}</span></div>
      </div>
      <AutomationTaskBadge task={task} />
      {controls}
    </header> : null}
    {error ? <div className="automation-error" role="alert"><span>{error}</span><button aria-label="关闭错误提示" onClick={() => setError('')}><X /></button></div> : null}
    {notice ? <div className="automation-notice" role="status"><span>{notice}</span><button aria-label="关闭提示" onClick={() => setNotice('')}><X /></button></div> : null}
    {!task && !error ? <div className="automation-empty"><LoaderCircle className="automation-spin" /><span>正在加载自动化…</span></div> : null}
    {task ? <>
      <nav className="automation-tabs" aria-label="自动化详情"><button className={tab === 'overview' ? 'active' : ''} aria-current={tab === 'overview' ? 'page' : undefined} onClick={() => setTab('overview')}>概述</button><button className={tab === 'runs' ? 'active' : ''} aria-current={tab === 'runs' ? 'page' : undefined} onClick={() => setTab('runs')}>执行历史</button></nav>
      <div className="automation-detail-scroll"><div className="automation-detail">
        {task.scheduler_error ? <div className="automation-error"><span>{task.scheduler_error}</span><button disabled={busy} onClick={() => void action('sync')}><RefreshCw />重新同步</button></div> : null}
        {tab === 'runs' ? <RunHistory expanded={expandedRun} onExpandedChange={setExpandedRun} key={task.id} taskId={task.id} visible={pollingVisible} refreshKey={refreshKey} onOpenSession={onOpenSession} /> : <div className="automation-overview">
          <div className="automation-metrics">
            <div><span>{task.kind === 'manual' ? <MousePointerClick /> : <CalendarClock />}{task.kind === 'manual' ? '执行方式' : '运行计划'}</span><strong>{task.kind === 'manual' ? '手动运行' : scheduleName(task.cron, task.interval_seconds)}</strong><small>{task.max_concurrent_runs === 1 ? '禁止重叠执行，执行中自动跳过' : `最多同时执行 ${task.max_concurrent_runs} 个任务`}</small></div>
            {task.kind === 'manual' ? <div><span><FileText />执行变量</span><strong>{variableNames(task.prompt_bindings).length} 个变量</strong><small>{task.prompt_bindings?.length ? '每次运行前填写' : '点击运行即可执行'}</small></div> : <div><span><Clock3 />下次运行</span><strong>{task.enabled ? task.interval_seconds ? '等待系统触发' : dateTime(task.next_run_at) : '已暂停'}</strong><small>{task.interval_seconds ? task.max_concurrent_runs === 1 ? '结束后等待下一次触发' : '达到并发上限时跳过，不排队' : task.cron}</small></div>}
            <div><span><History />最近运行</span><strong>{task.last_run ? dateTime(task.last_run.started_at) : '暂无记录'}</strong>{task.last_run ? <RunBadge run={task.last_run} /> : <small>等待首次运行</small>}</div>
          </div>
          {task.kind === 'manual' && task.prompt_bindings?.length ? <section className="automation-variables" aria-label="任务变量"><strong>执行时填写的变量</strong><div className="automation-variable-list">{variableNames(task.prompt_bindings).map(name => <code key={name}>{name}</code>)}</div></section> : null}
          <dl className="automation-configuration"><div><dt>Agent</dt><dd><AgentIcon agentId={task.agent} />{agentNames[task.agent]}</dd></div><div><dt>执行权限</dt><dd>{task.yolo ? 'Yolo / Full Access' : '标准权限'}</dd></div><div><dt>失败提醒</dt><dd>{task.failure_notification === 'feishu' ? '飞书 Bot' : task.failure_notification === 'wechat' ? '微信 Bot' : '不提醒'}</dd></div><div><dt>工作区方式</dt><dd>{workspaceNames[task.workspace_mode]}</dd></div><div><dt>任务结束清理</dt><dd>{task.workspace_mode === 'new_worktree' ? '强制清理 Worktree' : task.workspace_mode === 'temporary' ? '自动清理临时目录' : '—'}</dd></div><div><dt>基础分支</dt><dd>{task.workspace_mode === 'existing' || task.workspace_mode === 'temporary' ? '—' : <code>{task.base_branch}</code>}</dd></div><div><dt>更新时间</dt><dd>{dateTime(task.updated_at)}</dd></div><div className="automation-full-width"><dt>任务 ID</dt><dd><code>{task.id}</code></dd></div><div className="automation-full-width"><dt>{task.workspace_mode === 'temporary' ? '关联项目工作区' : '工作区'}</dt><dd><code>{task.workspace_path}</code></dd></div></dl>
          <section className="automation-prompt-preview"><h3><FileText />任务内容</h3><MarkdownContent readOnly text={task.prompt} className="automation-prompt-markdown" /></section>{task.precheck_command ? <section className="automation-prompt-preview automation-command-preview"><h3><Terminal />执行前检查<small>{task.precheck_timeout_seconds} 秒超时</small></h3><pre>{task.precheck_command}</pre></section> : null}
        </div>}
      </div></div>
    </> : null}
    {runTask ? <ManualRunDialog task={runTask} onClose={() => setRunTask(undefined)} onStarted={runId => {
      setRunTask(undefined); setTab('runs'); setRefreshKey(value => value + 1);
      setNotice(runId ? '已提交运行，等待 Runner 启动。' : '任务仍在执行，已跳过本次触发。'); void refresh(true);
    }} /> : null}
    {editorOpen && task ? <AutomationEditor task={task} project={project} agents={agents} timezone={status?.timezone} onClose={() => setEditorOpen(false)} onSaved={(updated) => { setEditorOpen(false); apply(updated); void refresh(true); }} /> : null}
    {confirmationDialog}
  </section>;
}
