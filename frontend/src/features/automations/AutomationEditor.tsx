import { t } from '../../lib/i18n';
import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { editor } from 'monaco-editor';
import { CalendarClock, LoaderCircle, MousePointerClick, Settings2, X } from 'lucide-react';
import type { AowAgent } from '../agents/types';
import { aowAgentType } from '../agents/agentTypes';
import type { AowProject } from '../../aow/types';
import { notificationsApi } from '../notifications/api';
import { agentNames, errorMessage } from './presentation';
import { automationApi } from './api';
import type { AutomationTask, TaskInput, TaskKind } from './types';

import { WorkspaceSelect } from '../workspaces/WorkspaceSelect';
import { workspaceConfig } from '../workspaces/types';

import { identifyVariables, variableNames } from './variables';
import './automations.css';

const Editor = lazy(() => import('../editor/MonacoEditor'));

type Cadence = 'interval' | 'hourly' | 'daily' | 'weekdays' | 'weekly' | 'custom';
function parseSchedule(cron: string): { cadence: Cadence; time: string; weekday: string; minute: string } {
  const [minute, hour, day, month, weekday] = cron.split(/\s+/);
  if (/^\d+$/.test(minute) && day === '*' && month === '*') {
    if (hour === '*' && weekday === '*') return { cadence: 'hourly', time: '09:00', weekday: '1', minute };
    if (/^\d+$/.test(hour)) {
      const cadence = weekday === '*' ? 'daily' : weekday === '1-5' ? 'weekdays' : /^[0-7]$/.test(weekday) ? 'weekly' : 'custom';
      return { cadence, time: `${hour.padStart(2, '0')}:${minute.padStart(2, '0')}`, weekday: /^[0-7]$/.test(weekday) ? weekday === '7' ? '0' : weekday : '1', minute };
    }
  }
  return { cadence: 'custom', time: '09:00', weekday: '1', minute: '0' };
}

export function AutomationEditor({ task, kind = 'scheduled', initialWorkspaceMode = 'new_worktree', project, agents, timezone, onClose, onSaved }: {
  task?: AutomationTask;
  kind?: TaskKind;
  initialWorkspaceMode?: TaskInput['workspace_mode'];
  project: AowProject;
  agents: AowAgent[];
  timezone?: string;
  onClose: () => void;
  onSaved: (task: AutomationTask) => void;
}) {
  const initialAgent = agents.find(agent => agent.available && aowAgentType(agent));
  const initialWorktree = project.worktrees.find((worktree) => worktree.is_main) ?? project.worktrees[0];
  const [draft, setDraft] = useState<TaskInput>(() => task ? { ...task, kind: task.kind ?? 'scheduled', prompt_bindings: task.prompt_bindings ?? [], failure_notification: task.failure_notification ?? null } : {
    kind, prompt_bindings: [], name: '', prompt: '', agent: initialAgent ? aowAgentType(initialAgent)! : 'codex', agent_id: initialAgent?.id,
    project_id: project.id, workspace_mode: initialWorkspaceMode, workspace_path: initialWorktree?.path ?? '',
    cleanup_worktree: true, base_branch: '', cron: '0 9 * * *', interval_seconds: null, max_concurrent_runs: 1, enabled: true, yolo: true, failure_notification: null,
  });
  const [schedule, setSchedule] = useState(() => ({ ...parseSchedule(draft.cron), ...(draft.interval_seconds ? { cadence: 'interval' as Cadence } : {}) }));
  const [lastConcurrentRuns, setLastConcurrentRuns] = useState(() => draft.max_concurrent_runs > 1 ? draft.max_concurrent_runs : 3);
  const initialSeconds = draft.interval_seconds;
  const initialUnit = initialSeconds ? initialSeconds % 3600 === 0 ? 3600 : initialSeconds % 60 === 0 ? 60 : 1 : 60;
  const [intervalUnit, setIntervalUnit] = useState(initialUnit);
  const [intervalAmount, setIntervalAmount] = useState(initialSeconds ? String(initialSeconds / initialUnit) : '');
  const [customCron, setCustomCron] = useState(draft.cron);
  const [busy, setBusy] = useState(false);
  const [workspaceReady, setWorkspaceReady] = useState(false);
  const [error, setError] = useState('');
  const manual = draft.kind === 'manual';
  const names = variableNames(draft.prompt_bindings);
  const [botStatus, setBotStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [availableBots, setAvailableBots] = useState<string[]>([]);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const promptEditorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const update = <K extends keyof TaskInput>(key: K, value: TaskInput[K]) => setDraft((current) => ({ ...current, [key]: value }));

  useEffect(() => {
    let active = true;
    void notificationsApi.notificationSettings().then(settings => {
      if (active) {
        setAvailableBots(settings.im.providers.filter(provider => provider.provider === 'wechat' || provider.secret_configured).map(provider => provider.provider));
        setBotStatus('ready');
      }
    }).catch(() => { if (active) setBotStatus('error'); });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal(); nameRef.current?.focus();
    return () => { dialog?.close(); };
  }, []);

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (busy || !workspaceReady) return;
    if (!draft.prompt.trim()) {
      setError(t("automations.enter_task_content"));
      promptEditorRef.current?.focus();
      return;
    }
    if (new TextEncoder().encode(draft.prompt).length > 65536) {
      setError(t("automations.task_content_must_not_exceed_64_kib"));
      promptEditorRef.current?.focus();
      return;
    }
    const [hour, minute] = schedule.time.split(':').map(Number);
    const cron = schedule.cadence === 'custom' ? customCron.trim()
      : schedule.cadence === 'hourly' ? `${Number(schedule.minute)} * * * *`
        : `${minute} ${hour} * * ${schedule.cadence === 'daily' ? '*' : schedule.cadence === 'weekdays' ? '1-5' : schedule.weekday}`;
    const input = { ...draft, ...workspaceConfig(draft), enabled: manual ? true : draft.enabled, project_id: project.id, name: draft.name.trim(), cleanup_worktree: draft.workspace_mode === 'new_worktree' ? true : draft.cleanup_worktree, cron: manual || schedule.cadence === 'interval' ? '' : cron, interval_seconds: !manual && schedule.cadence === 'interval' ? Number(intervalAmount) * intervalUnit : null };
    setBusy(true); setError('');
    try { onSaved(task ? await automationApi.update(task.id, input, task.revision) : await automationApi.create(input)); }
    catch (reason) { setError(errorMessage(reason)); setBusy(false); }
  };

  return createPortal(<dialog ref={dialogRef} className="automation-editor" aria-labelledby="automation-editor-title"
    onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }} onKeyDown={(event) => event.stopPropagation()}
    onDoubleClick={(event) => event.stopPropagation()} onDragStart={(event) => event.stopPropagation()}>
    <form onSubmit={(event) => void save(event)}>
      <header><div>{manual ? <MousePointerClick /> : <CalendarClock />}<h2 id="automation-editor-title">{task ? manual ? t("automations.edit_manual_task") : t("automations.edit_automation") : manual ? t("automations.create_manual_task") : t("automations.create_automation")}</h2></div><button type="button" disabled={busy} aria-label={t("aow.close")} onClick={onClose}><X /></button></header>
      <div className="automation-editor-body">
        <section className="automation-prompt-editor">
          <label htmlFor="automation-name">{t("automations.name")}</label>
          <input id="automation-name" ref={nameRef} className="automation-name-input" placeholder={t("automations.for_example_daily_code_review")} required maxLength={200} value={draft.name} onChange={(event) => update('name', event.target.value)} />
          <div className="automation-prompt-label"><span id="automation-prompt-label">{t("automations.task_content")}</span><small>Markdown</small></div>
          <div className="automation-prompt-input" role="group" aria-labelledby="automation-prompt-label">
            <div className="automation-prompt-monaco">
              <Suspense fallback={<span className="automation-hint">{t("aow.loading_editor")}</span>}>
                <Editor language="markdown" value={draft.prompt} saveViewState={false}
                  loading={<span className="automation-hint">{t("aow.loading_editor")}</span>}
                  options={{
                    ariaLabel: t("automations.task_content"), placeholder: t("automations.describe_the_task_relevant_files_and_expected_results_for"),
                    automaticLayout: true, readOnly: busy, minimap: { enabled: false },
                    fontSize: 13, lineHeight: 22, lineNumbersMinChars: 3, wordWrap: 'on',
                    scrollBeyondLastLine: false, padding: { top: 10, bottom: 10 },
                    quickSuggestions: false, wordBasedSuggestions: 'off', suggestOnTriggerCharacters: false,
                  }}
                  onMount={instance => { promptEditorRef.current = instance; }}
                  onChange={value => { const prompt = value ?? ''; setDraft(current => prompt === current.prompt ? current : { ...current, prompt, prompt_bindings: current.kind === 'manual' ? identifyVariables(prompt) : [] }); }} />
              </Suspense>
            </div>
          </div>
          {manual ? <section className="automation-variables" aria-label={t("automations.task_variables")} aria-live="polite">
            <strong>{t("automations.variables_provided_at_runtime")} <small>{names.length}</small></strong>
            <p className="automation-hint">{t("automations.use")} {t("automations.variablename", { variableName: '{{variableName}}' })} {t("automations.to_insert_variables_enter_each_distinct_variable_only_once")}</p>
            {names.length ? <div className="automation-variable-list">{names.map(name => <code key={name}>{name}</code>)}</div> : <p className="automation-hint">{t("automations.no_variables_the_task_will_run_directly")}</p>}
          </section> : null}
        </section>
        <aside className="automation-settings">
          <h3><Settings2 />{t("automations.execution_configuration")}</h3>
          <div className="automation-agent-setting"><span>Agent</span><div className="automation-agent-row"><select aria-label="Agent" value={draft.agent_id ?? draft.agent} onChange={event => {
            const selected = agents.find(agent => agent.id === event.target.value);
            const type = selected && aowAgentType(selected);
            if (selected && type) setDraft(current => ({ ...current, agent: type, agent_id: selected.id }));
          }}>
            {!agents.some(agent => agent.id === (draft.agent_id ?? draft.agent)) && <option value={draft.agent_id ?? draft.agent} disabled>{draft.agent_id ?? agentNames[draft.agent]} {t("automations.unavailable")}</option>}
            {agents.filter(agent => aowAgentType(agent)).map(agent => <option key={agent.id} value={agent.id} disabled={!agent.available}>{agent.display_name}{agent.available ? '' : t("automations.not_installed")}</option>)}
          </select><label className="automation-yolo-option"><input type="checkbox" checked={draft.yolo} onChange={(event) => update('yolo', event.target.checked)} />{draft.agent === 'pi' ? t("automations.trust_project_configuration") : t("automations.unrestricted_mode")}</label></div></div>
          {draft.agent === 'pi' ? <p className="automation-hint">{t("automations.allow_pi_to_load_project_configuration_and_extensions_tool")}</p> : null}
          <WorkspaceSelect project={project} value={draft} disabled={busy} allowDynamic={manual}
            onChange={workspace => setDraft(current => ({ ...current, ...workspace }))} onValidityChange={setWorkspaceReady} />
          {draft.workspace_mode === 'temporary' ? <p className="automation-hint">{t("automations.delete_the_temporary_workspace_after_completion_agent_sessions_for")}</p> : null}
          {draft.workspace_mode === 'new_worktree' ? <p className="automation-hint">{t("automations.force_cleanup_of_the_worktree_after_completion_discarding_uncommitted")}</p> : null}
          <div className="automation-settings-divider" />
          <div className="automation-schedule-heading"><h3>{manual ? <MousePointerClick /> : <CalendarClock />}{manual ? t("automations.manual_execution") : t("automations.schedule")}</h3><label className="automation-checkbox"><input type="checkbox" checked={draft.max_concurrent_runs === 1} onChange={(event) => { if (event.target.checked) { if (draft.max_concurrent_runs > 1) setLastConcurrentRuns(draft.max_concurrent_runs); update('max_concurrent_runs', 1); } else update('max_concurrent_runs', lastConcurrentRuns); }} />{t("automations.prevent_overlapping_runs")}</label></div>
          {draft.max_concurrent_runs > 1 ? <label><span>{t("automations.maximum_concurrent_runs")}</span><input aria-label={t("automations.maximum_concurrent_runs")} type="number" required min="2" max="10" step="1" value={draft.max_concurrent_runs} onChange={(event) => { const value = Number(event.target.value); setLastConcurrentRuns(value); update('max_concurrent_runs', value); }} /><small>{t("automations.skip_the_trigger_when_the_limit_is_reached_no")}</small></label> : null}
          {!manual ? <>
          <label><span>{t("automations.frequency")}</span><select aria-label={t("automations.schedule")} value={schedule.cadence} onChange={(event) => setSchedule((current) => ({ ...current, cadence: event.target.value as Cadence }))}>
            <option value="interval">{t("automations.fixed_interval")}</option><option value="hourly">{t("automations.hourly")}</option><option value="daily">{t("automations.daily")}</option><option value="weekdays">{t("automations.weekdays")}</option><option value="weekly">{t("automations.weekly")}</option><option value="custom">{t("automations.custom_cron")}</option>
          </select></label>
          {schedule.cadence === 'interval' ? <div className="automation-interval-fields">
            <label><span>{t("automations.interval")}</span><input aria-label={t("automations.run_interval")} type="number" required min="1" max={Math.floor(2678400 / intervalUnit)} step="1" placeholder={t("automations.enter_interval")} value={intervalAmount} onChange={(event) => setIntervalAmount(event.target.value)} /></label>
            <label><span>{t("automations.unit")}</span><select aria-label={t("automations.interval_unit")} value={intervalUnit} onChange={(event) => setIntervalUnit(Number(event.target.value))}><option value={1}>{t("automations.seconds")}</option><option value={60}>{t("automations.minutes")}</option><option value={3600}>{t("automations.hours")}</option></select></label>
          </div> : schedule.cadence === 'custom' ? <label><span>{t("automations.cron_expression")}</span><input aria-label={t("automations.cron_expression")} required value={customCron} onChange={(event) => setCustomCron(event.target.value)} placeholder="0 9 * * 1-5" /><small>{t("automations.minute_hour_day_month_weekday_supports_lists_ranges_and")}</small></label>
            : schedule.cadence === 'hourly' ? <label><span>{t("automations.minute_of_each_hour")}</span><input aria-label={t("automations.minute_of_each_hour")} type="number" required min="0" max="59" value={schedule.minute} onChange={(event) => setSchedule((current) => ({ ...current, minute: event.target.value }))} /></label>
              : <label><span>{t("automations.time")}</span><input aria-label={t("automations.time")} type="time" required value={schedule.time} onChange={(event) => setSchedule((current) => ({ ...current, time: event.target.value }))} /></label>}
          {schedule.cadence === 'weekly' ? <label><span>{t("automations.day_of_week")}</span><select aria-label={t("automations.day_of_week")} value={schedule.weekday} onChange={(event) => setSchedule((current) => ({ ...current, weekday: event.target.value }))}>{[t("automations.sun"), t("automations.mon"), t("automations.tue"), t("automations.wed"), t("automations.thu"), t("automations.fri"), t("automations.sat")].map((day, index) => <option key={day} value={index}>{t("automations.day_of_week")}{day}</option>)}</select></label> : null}
          <p className="automation-hint">{schedule.cadence === 'interval' ? t("automations.wait_one_full_interval_after_enabling_then_trigger_at") : t("automations.run_in_the_local_time_zone", { value0: timezone ? `：${timezone}` : '' })}</p>
          </> : <p className="automation-hint">{t("automations.click_run_and_enter_variables_to_execute_the_task")}</p>}
          <div className="automation-settings-divider" />
          <p className="automation-hint">{draft.max_concurrent_runs === 1 ? t("automations.skip_the_trigger_if_the_previous_run_is_still") : t("automations.run_up_to_tasks_concurrently_skip_triggers_when_the", { value0: draft.max_concurrent_runs })}</p>
          <label><span>{t("automations.failure_notifications")}</span><select aria-label={t("automations.failure_notifications")} value={draft.failure_notification ?? ''} disabled={busy} onChange={event => update('failure_notification', event.target.value === 'feishu' || event.target.value === 'wechat' ? event.target.value : null)}>
            <option value="">{t("automations.no_notifications")}</option>
            {(['feishu', 'wechat'] as const).map(provider => <option key={provider} value={provider} disabled={!availableBots.includes(provider)}>{provider === 'feishu' ? t("automations.feishu") : t("automations.wechat")} Bot{botStatus === 'ready' && !availableBots.includes(provider) ? t("automations.not_configured_in_this_environment") : ''}</option>)}
          </select></label>
          <p className="automation-hint">{botStatus === 'loading' ? t("automations.checking_im_bot_configuration") : botStatus === 'error' ? t("automations.unable_to_read_im_bot_configuration_existing_notification_settings") : draft.failure_notification && !availableBots.includes(draft.failure_notification) ? t("automations.the_bot_is_not_configured_in_this_environment_notification", { value0: draft.failure_notification === 'feishu' ? t("automations.feishu") : t("automations.wechat") }) : !availableBots.length ? t("automations.configure_a_feishu_or_wechat_bot_in_settings_im") : t("automations.notify_via_the_selected_bot_when_a_task_fails")}</p>
        </aside>
      </div>
      {error ? <div className="automation-error" role="alert">{error}</div> : null}
      <footer>{!manual ? <label className="automation-checkbox"><input type="checkbox" checked={draft.enabled} onChange={(event) => update('enabled', event.target.checked)} />{t("automations.enable_automation")}</label> : <span />}<div><button type="button" disabled={busy} onClick={onClose}>{t("aow.cancel")}</button><button className="automation-primary" type="submit" disabled={busy || !workspaceReady || !agents.some((agent) => agent.id === (draft.agent_id ?? draft.agent) && agent.available)}>{busy ? <LoaderCircle className="automation-spin" /> : null}{busy ? t("automations.saving") : task ? t("automations.save_changes") : manual ? t("automations.create_manual_task") : t("automations.create_automation")}</button></div></footer>
    </form>
  </dialog>, document.body);
}
