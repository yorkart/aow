import { CalendarClock, ChevronRight, MousePointerClick } from 'lucide-react';
import { automationApi } from './api';
import { agentNames, dateTime, duration, runNames, scheduleName, workspaceNames } from './presentation';
import type { AutomationRun } from './types';
import { sessionsApi } from '../sessions/api';
import { AgentIcon } from '../agents/AgentIcon';
import { RunParameters } from './RunParameters';
import { MarkdownContent } from '../../components/MarkdownContent';
import type { AowAgentSession } from '../sessions/types';
import type { MobileNavigate } from '../../mobile/mobileState';
import { MobilePageHeader, MobileRefresh, MobileState } from '../../mobile/MobilePrimitives';
import { useMobileResource, useMobileScroll, type MobileRoute } from '../../mobile/mobileState';

function taskState(task: { scheduler_error: string | null; is_running: boolean; enabled: boolean; kind?: string }) {
  return task.scheduler_error ? '配置异常' : task.is_running ? '执行中' : task.kind === 'manual' ? '手动' : task.enabled ? '已启用' : '已暂停';
}

export function MobileAutomations({ route, projectId, visible, navigate, back }: {
  route: MobileRoute; projectId: string; visible: boolean; navigate: MobileNavigate; back: () => void;
}) {
  const tasks = useMobileResource(`automations.${projectId}`, () => automationApi.list(projectId), visible && !route.task);
  const scroll = useMobileScroll(`automations.${projectId}`, !!tasks.data);
  if (route.task) return <MobileAutomationTask route={route} navigate={navigate} back={back} />;
  return <section className="mobile-content-page">
    <MobilePageHeader title="自动化" subtitle="项目任务与执行记录" actions={<MobileRefresh reload={tasks.reload} loading={tasks.loading} />} />
    <div className="mobile-scroll" ref={scroll}>
      <MobileState loading={tasks.loading && !tasks.data} error={tasks.error} retry={tasks.reload} empty={tasks.data?.length === 0 ? '这个项目还没有自动化任务。' : undefined} />
      <div className="mobile-list">{tasks.data?.map((task) => <button className="mobile-list-row" key={task.id} onClick={() => navigate({ workspace: route.workspace, view: 'automations', task: task.id })}>
        <div className="mobile-row-icon">{task.kind === 'manual' ? <MousePointerClick size={21} /> : <CalendarClock size={21} />}</div><div className="mobile-row-main"><strong>{task.name}</strong><span>{task.kind === 'manual' ? '手动运行' : task.enabled ? scheduleName(task.cron, task.interval_seconds) : '已停用'} · {task.is_running ? '执行中' : task.last_run ? runNames[task.last_run.status] : '尚未执行'}</span></div><ChevronRight size={17} />
      </button>)}</div>
    </div>
  </section>;
}

function MobileAutomationTask({ route, navigate, back }: { route: MobileRoute; navigate: MobileNavigate; back: () => void }) {
  const task = useMobileResource(`task.${route.task}`, () => automationApi.detail(route.task!));
  const runs = useMobileResource(`runs.${route.task}`, () => automationApi.runs(route.task!));
  const scroll = useMobileScroll(`task.${route.task}`, !!task.data);
  if (route.run) return <MobileAutomationRun route={route} back={back} />;
  return <section className="mobile-content-page">
    <MobilePageHeader title={task.data?.name ?? '自动化任务'} back={back} actions={<MobileRefresh reload={() => { task.reload(); runs.reload(); }} loading={task.loading || runs.loading} />} />
    <div className="mobile-scroll" ref={scroll}>
      <MobileState loading={task.loading && !task.data} error={task.error} retry={task.reload} />
      {task.data && <><section className="mobile-automation-card"><h3>任务配置</h3><dl className="mobile-automation-details">
        <div><dt>任务 ID</dt><dd><code>{task.data.id}</code></dd></div>
        <div><dt>项目</dt><dd>{task.data.project_name}<small><code>{task.data.project_id}</code></small></dd></div>
        <div><dt>状态</dt><dd>{taskState(task.data)}</dd></div>
        <div><dt>Agent</dt><dd>{agentNames[task.data.agent]}</dd></div>
        <div><dt>执行权限</dt><dd>{task.data.yolo ? 'Yolo / Full Access' : '标准权限'}</dd></div>
        <div><dt>失败提醒</dt><dd>{task.data.failure_notification === 'feishu' ? '飞书 Bot' : '不提醒'}</dd></div>
        {task.data.kind === 'manual' ? <div><dt>执行方式</dt><dd>手动运行</dd></div> : <div><dt>运行计划</dt><dd>{scheduleName(task.data.cron, task.data.interval_seconds)}<small><code>{task.data.interval_seconds ? `${task.data.interval_seconds} 秒` : task.data.cron}</code></small></dd></div>}
        <div><dt>最大并发</dt><dd>{task.data.max_concurrent_runs === 1 ? '1（禁止重叠执行）' : task.data.max_concurrent_runs}</dd></div>
        {task.data.kind !== 'manual' ? <div><dt>下次运行</dt><dd>{task.data.enabled ? dateTime(task.data.next_run_at) : '已暂停'}</dd></div> : null}
        <div><dt>工作区方式</dt><dd>{workspaceNames[task.data.workspace_mode]}</dd></div>
        <div><dt>{task.data.workspace_mode === 'temporary' ? '关联项目工作区' : '工作区'}</dt><dd><code>{task.data.workspace_path}</code></dd></div>
        <div><dt>基础分支</dt><dd>{task.data.workspace_mode === 'existing' || task.data.workspace_mode === 'temporary' ? '—' : <code>{task.data.base_branch}</code>}</dd></div>
        <div><dt>结束后清理</dt><dd>{task.data.workspace_mode === 'new_worktree' ? '强制清理 Worktree' : task.data.workspace_mode === 'temporary' ? '自动清理临时目录' : '—'}</dd></div>
        <div><dt>创建时间</dt><dd>{dateTime(task.data.created_at)}</dd></div>
        <div><dt>更新时间</dt><dd>{dateTime(task.data.updated_at)}</dd></div>
      </dl>{task.data.scheduler_error && <p className="mobile-inline-error">{task.data.scheduler_error}</p>}</section>
      <section className="mobile-automation-card"><h3>任务内容</h3><MarkdownContent readOnly text={task.data.prompt} className="mobile-markdown mobile-automation-markdown" /></section>
      {task.data.precheck_command && <section className="mobile-automation-card"><h3>执行前检查 <small>{task.data.precheck_timeout_seconds} 秒超时</small></h3><pre className="mobile-automation-command">{task.data.precheck_command}</pre></section>}</>}
      <h3 className="mobile-section-label">执行历史</h3>
      <MobileState loading={runs.loading && !runs.data} error={runs.error} retry={runs.reload} empty={runs.data?.length === 0 ? '暂无执行记录。' : undefined} />
      <div className="mobile-list">{runs.data?.map((run) => <button className="mobile-list-row" key={run.id}
        onClick={() => navigate({ ...route, run: run.id })}>
        <div className="mobile-row-icon"><AgentIcon agentId={run.agent} /></div><div className="mobile-row-main"><strong>{runNames[run.status]}</strong><span>{dateTime(run.started_at)} · {duration(run.duration_ms)}</span></div><ChevronRight size={17} />
      </button>)}</div>
      {runs.data?.length === 50 && <p className="mobile-reader-meta">展示最近 50 次执行。</p>}
    </div>
  </section>;
}

function MobileAutomationRun({ route, back }: { route: MobileRoute; back: () => void }) {
  const run = useMobileResource(`run.${route.task}.${route.run}`, () => automationApi.runDetail(route.task!, route.run!));
  const session = useMobileResource(`run-session.${route.task}.${route.run}`, () => sessionsApi.automationRunSession(route.task!, route.run!), !!run.data?.session_id);
  const scroll = useMobileScroll(`run.${route.task}.${route.run}`, !!run.data);
  return <section className="mobile-content-page">
    <MobilePageHeader title={run.data?.task_name ?? '执行详情'} subtitle={run.data ? `Run ${run.data.id}` : undefined} back={back} actions={<MobileRefresh reload={() => { run.reload(); session.reload(); }} loading={run.loading || session.loading} />} />
    <div className="mobile-scroll" ref={scroll}><MobileState loading={run.loading && !run.data} error={run.error} retry={run.reload} />
      {run.data && <><section className="mobile-automation-card"><h3>执行信息</h3><dl className="mobile-automation-details">
        <div><dt>Run ID</dt><dd><code>{run.data.id}</code></dd></div>
        <div><dt>任务 ID</dt><dd><code>{run.data.task_id}</code></dd></div>
        <div><dt>状态</dt><dd>{runNames[run.data.status]}</dd></div>
        <div><dt>触发方式</dt><dd>{run.data.source === 'manual' ? '手动运行' : '计划运行'}</dd></div>
        <div><dt>开始时间</dt><dd>{dateTime(run.data.started_at)}</dd></div>
        <div><dt>结束时间</dt><dd>{dateTime(run.data.finished_at)}</dd></div>
        <div><dt>耗时</dt><dd>{duration(run.data.duration_ms)}</dd></div>
        <div><dt>工作区</dt><dd><code>{run.data.workspace_path ?? '—'}</code></dd></div>
        <div><dt>分支</dt><dd><code>{run.data.branch ?? '—'}</code></dd></div>
        <div><dt>Session ID</dt><dd><code>{run.data.session_id ?? '—'}</code></dd></div>
        <div><dt>退出码</dt><dd>{run.data.exit_code ?? '—'}</dd></div>
      </dl>{run.data.message && <p className="mobile-automation-run-message">{run.data.message}</p>}</section>
      <RunParameters run={run.data} />
      <h3 className="mobile-section-label mobile-automation-result-label">本次执行内容</h3>
      {run.data.session_id ? session.data ? <MobileAutomationRunResult run={run.data} session={session.data} workspace={run.data.workspace_path ?? route.workspace} />
        : <><MobileState loading={session.loading} error={session.error} retry={session.reload} />{session.error ? <RunOutput run={run.data} /> : null}</>
        : <RunOutput run={run.data} />}</>}
    </div>
  </section>;
}

function MobileAutomationRunResult({ run, session, workspace }: { run: AutomationRun; session: AowAgentSession; workspace: string }) {
  const snapshot = useMobileResource(`automation-run-snapshot.${workspace}.${session.id}`, () => sessionsApi.agentSessionSnapshot(session, workspace));
  const results = snapshot.data?.turns.flatMap((turn) => turn.final ? [{ id: turn.id, message: turn.final }] : []) ?? [];
  return <section className="mobile-automation-card mobile-automation-result">
    <MobileState loading={snapshot.loading && !snapshot.data} error={snapshot.error} retry={snapshot.reload} />
    {results.map((result) => <article key={result.id}><AgentIcon agentId={session.agent} /><MarkdownContent readOnly text={result.message.text} className="mobile-markdown" /></article>)}
    {snapshot.data && !results.length ? <RunOutput run={run} embedded /> : null}
  </section>;
}

function RunOutput({ run, embedded = false }: { run: AutomationRun; embedded?: boolean }) {
  const output = useMobileResource(`output.${run.id}`, () => automationApi.runOutput(run.task_id, run.id, 'stdio'));
  const content = <><MobileState loading={output.loading} error={output.error} retry={output.reload} empty={output.data === '' ? '暂无执行内容。' : undefined} />{output.data ? <pre className="mobile-code mobile-automation-output">{output.data}</pre> : null}</>;
  return embedded ? content : <section className="mobile-automation-card mobile-automation-result">{content}</section>;
}
