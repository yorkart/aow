import { t } from '../../lib/i18n';
import { CalendarClock, ChevronRight, MousePointerClick } from 'lucide-react';
import { automationApi } from './api';
import { agentNames, dateTime, duration, executionPermission, runNames, scheduleName, workspaceNames } from './presentation';
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
  return task.scheduler_error ? t("automations.configuration_error") : task.is_running ? t("aow.running") : task.kind === 'manual' ? t("automations.manual") : task.enabled ? t("automations.enabled") : t("automations.paused");
}

export function MobileAutomations({ route, projectId, visible, navigate, back }: {
  route: MobileRoute; projectId: string; visible: boolean; navigate: MobileNavigate; back: () => void;
}) {
  const tasks = useMobileResource(`automations.${projectId}`, () => automationApi.list(projectId), visible && !route.task);
  const scroll = useMobileScroll(`automations.${projectId}`, !!tasks.data);
  if (route.task) return <MobileAutomationTask route={route} navigate={navigate} back={back} />;
  return <section className="mobile-content-page">
    <MobilePageHeader title={t("aow.automations")} subtitle={t("automations.project_tasks_and_run_history")} actions={<MobileRefresh reload={tasks.reload} loading={tasks.loading} />} />
    <div className="mobile-scroll" ref={scroll}>
      <MobileState loading={tasks.loading && !tasks.data} error={tasks.error} retry={tasks.reload} empty={tasks.data?.length === 0 ? t("automations.this_project_has_no_automations_yet") : undefined} />
      <div className="mobile-list">{tasks.data?.map((task) => <button className="mobile-list-row" key={task.id} onClick={() => navigate({ workspace: route.workspace, view: 'automations', task: task.id })}>
        <div className="mobile-row-icon">{task.kind === 'manual' ? <MousePointerClick size={21} /> : <CalendarClock size={21} />}</div><div className="mobile-row-main"><strong>{task.name}</strong><span>{task.kind === 'manual' ? t("automations.manual_run") : task.enabled ? scheduleName(task.cron, task.interval_seconds) : t("automations.disabled")} · {task.is_running ? t("aow.running") : task.last_run ? runNames[task.last_run.status] : t("automations.never_run")}</span></div><ChevronRight size={17} />
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
    <MobilePageHeader title={task.data?.name ?? t("aow.automated_tasks")} back={back} actions={<MobileRefresh reload={() => { task.reload(); runs.reload(); }} loading={task.loading || runs.loading} />} />
    <div className="mobile-scroll" ref={scroll}>
      <MobileState loading={task.loading && !task.data} error={task.error} retry={task.reload} />
      {task.data && <><section className="mobile-automation-card"><h3>{t("automations.task_configuration")}</h3><dl className="mobile-automation-details">
        <div><dt>{t("automations.task_id")}</dt><dd><code>{task.data.id}</code></dd></div>
        <div><dt>{t("automations.project")}</dt><dd>{task.data.project_name}<small><code>{task.data.project_id}</code></small></dd></div>
        <div><dt>{t("automations.status")}</dt><dd>{taskState(task.data)}</dd></div>
        <div><dt>Agent</dt><dd>{agentNames[task.data.agent]}</dd></div>
        <div><dt>{t("automations.execution_permissions")}</dt><dd>{executionPermission(task.data.agent, task.data.yolo)}</dd></div>
        <div><dt>{t("automations.failure_notifications")}</dt><dd>{task.data.failure_notification === 'feishu' ? t("automations.feishu_bot") : task.data.failure_notification === 'wechat' ? t("automations.wechat_bot") : t("automations.no_notifications")}</dd></div>
        {task.data.kind === 'manual' ? <div><dt>{t("automations.execution_method")}</dt><dd>{t("automations.manual_run")}</dd></div> : <div><dt>{t("automations.schedule")}</dt><dd>{scheduleName(task.data.cron, task.data.interval_seconds)}<small><code>{task.data.interval_seconds ? t("aow.seconds", { count: task.data.interval_seconds, value0: task.data.interval_seconds }) : task.data.cron}</code></small></dd></div>}
        <div><dt>{t("automations.maximum_concurrency")}</dt><dd>{task.data.max_concurrent_runs === 1 ? t("automations.1_no_overlapping_runs") : task.data.max_concurrent_runs}</dd></div>
        {task.data.kind !== 'manual' ? <div><dt>{t("automations.next_run")}</dt><dd>{task.data.enabled ? dateTime(task.data.next_run_at) : t("automations.paused")}</dd></div> : null}
        <div><dt>{t("automations.workspace_mode")}</dt><dd>{workspaceNames[task.data.workspace_mode]}</dd></div>
        {task.data.workspace_mode === 'existing' && <div><dt>{t("automations.workspace")}</dt><dd><code>{task.data.workspace_path}</code></dd></div>}
        <div><dt>{t("automations.base_branch")}</dt><dd>{task.data.workspace_mode !== 'new_worktree' ? '—' : <code>{task.data.base_branch}</code>}</dd></div>
        <div><dt>{t("automations.cleanup_after_run")}</dt><dd>{task.data.workspace_mode === 'new_worktree' ? t("automations.force_worktree_cleanup") : task.data.workspace_mode === 'temporary' ? t("automations.automatically_clean_up_temporary_directory") : '—'}</dd></div>
        <div><dt>{t("automations.created_at")}</dt><dd>{dateTime(task.data.created_at)}</dd></div>
        <div><dt>{t("automations.updated_at")}</dt><dd>{dateTime(task.data.updated_at)}</dd></div>
      </dl>{task.data.scheduler_error && <p className="mobile-inline-error">{task.data.scheduler_error}</p>}</section>
      <section className="mobile-automation-card"><h3>{t("automations.task_content")}</h3><MarkdownContent readOnly text={task.data.prompt} className="mobile-markdown mobile-automation-markdown" /></section>
      </>}
      <h3 className="mobile-section-label">{t("automations.run_history")}</h3>
      <MobileState loading={runs.loading && !runs.data} error={runs.error} retry={runs.reload} empty={runs.data?.length === 0 ? t("automations.no_runs_yet_2") : undefined} />
      <div className="mobile-list">{runs.data?.map((run) => <button className="mobile-list-row" key={run.id}
        onClick={() => navigate({ ...route, run: run.id })}>
        <div className="mobile-row-icon"><AgentIcon agentId={run.agent} /></div><div className="mobile-row-main"><strong>{runNames[run.status]}</strong><span>{dateTime(run.started_at)} · {duration(run.duration_ms)}</span></div><ChevronRight size={17} />
      </button>)}</div>
      {runs.data?.length === 50 && <p className="mobile-reader-meta">{t("automations.showing_the_latest_50_runs")}</p>}
    </div>
  </section>;
}

function MobileAutomationRun({ route, back }: { route: MobileRoute; back: () => void }) {
  const run = useMobileResource(`run.${route.task}.${route.run}`, () => automationApi.runDetail(route.task!, route.run!));
  const session = useMobileResource(`run-session.${route.task}.${route.run}`, () => sessionsApi.automationRunSession(route.task!, route.run!), !!run.data?.session_id);
  const scroll = useMobileScroll(`run.${route.task}.${route.run}`, !!run.data);
  return <section className="mobile-content-page">
    <MobilePageHeader title={run.data?.task_name ?? t("automations.run_details")} subtitle={run.data ? `Run ${run.data.id}` : undefined} back={back} actions={<MobileRefresh reload={() => { run.reload(); session.reload(); }} loading={run.loading || session.loading} />} />
    <div className="mobile-scroll" ref={scroll}><MobileState loading={run.loading && !run.data} error={run.error} retry={run.reload} />
      {run.data && <><section className="mobile-automation-card"><h3>{t("automations.run_information")}</h3><dl className="mobile-automation-details">
        <div><dt>{t("navigation.run_id")}</dt><dd><code>{run.data.id}</code></dd></div>
        <div><dt>{t("automations.task_id")}</dt><dd><code>{run.data.task_id}</code></dd></div>
        <div><dt>{t("automations.status")}</dt><dd>{runNames[run.data.status]}</dd></div>
        <div><dt>{t("automations.trigger_type")}</dt><dd>{run.data.source === 'manual' ? t("automations.manual_run") : t("automations.scheduled_run")}</dd></div>
        <div><dt>{t("automations.start_time")}</dt><dd>{dateTime(run.data.started_at)}</dd></div>
        <div><dt>{t("automations.end_time")}</dt><dd>{dateTime(run.data.finished_at)}</dd></div>
        <div><dt>{t("automations.duration")}</dt><dd>{duration(run.data.duration_ms)}</dd></div>
        <div><dt>{t("automations.workspace")}</dt><dd><code>{run.data.workspace_path ?? '—'}</code></dd></div>
        <div><dt>{t("automations.branch")}</dt><dd><code>{run.data.branch ?? '—'}</code></dd></div>
        <div><dt>{t("navigation.session_id")}</dt><dd><code>{run.data.session_id ?? '—'}</code></dd></div>
        <div><dt>{t("automations.exit_code")}</dt><dd>{run.data.exit_code ?? '—'}</dd></div>
      </dl>{run.data.message && <p className="mobile-automation-run-message">{run.data.message}</p>}</section>
      <RunParameters run={run.data} />
      <h3 className="mobile-section-label mobile-automation-result-label">{t("automations.content_for_this_run")}</h3>
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
  const content = <><MobileState loading={output.loading} error={output.error} retry={output.reload} empty={output.data === '' ? t("automations.no_run_content") : undefined} />{output.data ? <pre className="mobile-code mobile-automation-output">{output.data}</pre> : null}</>;
  return embedded ? content : <section className="mobile-automation-card mobile-automation-result">{content}</section>;
}
