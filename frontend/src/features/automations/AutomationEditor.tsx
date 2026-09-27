import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { editor } from 'monaco-editor';
import { CalendarClock, LoaderCircle, MousePointerClick, Settings2, Terminal, X } from 'lucide-react';
import type { AowAgent } from '../agents/types';
import type { AowProject } from '../../aow/types';
import { notificationsApi } from '../notifications/api';
import { agentNames, errorMessage } from './presentation';
import { automationApi } from './api';
import type { AutomationAgent, AutomationTask, TaskInput, TaskKind, WorkspaceMode } from './types';

import { identifyVariables, variableNames } from './variables';

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

export function AutomationEditor({ task, kind = 'scheduled', project, agents, timezone, onClose, onSaved }: {
  task?: AutomationTask;
  kind?: TaskKind;
  project: AowProject;
  agents: AowAgent[];
  timezone?: string;
  onClose: () => void;
  onSaved: (task: AutomationTask) => void;
}) {
  const initialWorktree = project.worktrees.find((worktree) => worktree.is_main) ?? project.worktrees[0];
  const [draft, setDraft] = useState<TaskInput>(() => task ? { ...task, kind: task.kind ?? 'scheduled', prompt_bindings: task.prompt_bindings ?? [], failure_notification: task.failure_notification ?? null } : {
    kind, prompt_bindings: [], name: '', prompt: '', agent: (agents.find((agent) => agent.available && Object.hasOwn(agentNames, agent.id))?.id as AutomationAgent | undefined) ?? 'codex',
    project_id: project.id, workspace_mode: 'new_worktree', workspace_path: initialWorktree?.path ?? '',
    cleanup_worktree: true, base_branch: initialWorktree?.branch || 'HEAD', cron: '0 9 * * *', interval_seconds: null, max_concurrent_runs: 1, enabled: true, yolo: true, precheck_command: '', precheck_timeout_seconds: 60, failure_notification: null,
  });
  const [schedule, setSchedule] = useState(() => ({ ...parseSchedule(draft.cron), ...(draft.interval_seconds ? { cadence: 'interval' as Cadence } : {}) }));
  const [lastConcurrentRuns, setLastConcurrentRuns] = useState(() => draft.max_concurrent_runs > 1 ? draft.max_concurrent_runs : 3);
  const initialSeconds = draft.interval_seconds;
  const initialUnit = initialSeconds ? initialSeconds % 3600 === 0 ? 3600 : initialSeconds % 60 === 0 ? 60 : 1 : 60;
  const [intervalUnit, setIntervalUnit] = useState(initialUnit);
  const [intervalAmount, setIntervalAmount] = useState(initialSeconds ? String(initialSeconds / initialUnit) : '');
  const [customCron, setCustomCron] = useState(draft.cron);
  const [busy, setBusy] = useState(false);
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
    if (busy) return;
    if (!draft.prompt.trim()) {
      setError('请输入任务内容。');
      promptEditorRef.current?.focus();
      return;
    }
    if (new TextEncoder().encode(draft.prompt).length > 65536) {
      setError('任务内容不能超过 64 KiB。');
      promptEditorRef.current?.focus();
      return;
    }
    const [hour, minute] = schedule.time.split(':').map(Number);
    const cron = schedule.cadence === 'custom' ? customCron.trim()
      : schedule.cadence === 'hourly' ? `${Number(schedule.minute)} * * * *`
        : `${minute} ${hour} * * ${schedule.cadence === 'daily' ? '*' : schedule.cadence === 'weekdays' ? '1-5' : schedule.weekday}`;
    const input = { ...draft, enabled: manual ? true : draft.enabled, project_id: project.id, name: draft.name.trim(), cleanup_worktree: draft.workspace_mode === 'new_worktree' ? true : draft.cleanup_worktree, cron: manual || schedule.cadence === 'interval' ? '' : cron, interval_seconds: !manual && schedule.cadence === 'interval' ? Number(intervalAmount) * intervalUnit : null };
    setBusy(true); setError('');
    try { onSaved(task ? await automationApi.update(task.id, input, task.revision) : await automationApi.create(input)); }
    catch (reason) { setError(errorMessage(reason)); setBusy(false); }
  };

  return createPortal(<dialog ref={dialogRef} className="automation-editor" aria-labelledby="automation-editor-title"
    onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }} onKeyDown={(event) => event.stopPropagation()}>
    <form onSubmit={(event) => void save(event)}>
      <header><div>{manual ? <MousePointerClick /> : <CalendarClock />}<h2 id="automation-editor-title">{task ? manual ? '编辑手动任务' : '编辑自动化' : manual ? '创建手动任务' : '创建自动化'}</h2></div><button type="button" disabled={busy} aria-label="关闭" onClick={onClose}><X /></button></header>
      <div className="automation-editor-body">
        <section className="automation-prompt-editor">
          <label htmlFor="automation-name">名称</label>
          <input id="automation-name" ref={nameRef} className="automation-name-input" placeholder="例如：每日代码检查" required maxLength={200} value={draft.name} onChange={(event) => update('name', event.target.value)} />
          <div className="automation-prompt-label"><span id="automation-prompt-label">任务内容</span><small>Markdown</small></div>
          <div className="automation-prompt-input" role="group" aria-labelledby="automation-prompt-label">
            <div className="automation-prompt-monaco">
              <Suspense fallback={<span className="automation-hint">正在加载编辑器…</span>}>
                <Editor language="markdown" theme="vs-dark" value={draft.prompt} saveViewState={false}
                  loading={<span className="automation-hint">正在加载编辑器…</span>}
                  options={{
                    ariaLabel: '任务内容', placeholder: '描述希望 Agent 执行的任务、关注的文件，以及预期的结果…',
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
          {manual ? <section className="automation-variables" aria-label="任务变量" aria-live="polite">
            <strong>执行时填写的变量 <small>{names.length}</small></strong>
            <p className="automation-hint">使用 {'{{变量名}}'} 插入变量；同名变量只需填写一次。</p>
            {names.length ? <div className="automation-variable-list">{names.map(name => <code key={name}>{name}</code>)}</div> : <p className="automation-hint">暂无变量，运行时将直接执行。</p>}
          </section> : null}
        </section>
        <aside className="automation-settings">
          <h3><Settings2 />执行配置</h3>
          <div className="automation-agent-setting"><span>Agent</span><div className="automation-agent-row"><select aria-label="Agent" value={draft.agent} onChange={(event) => update('agent', event.target.value as AutomationAgent)}>
            {Object.entries(agentNames).map(([id, name]) => <option key={id} value={id} disabled={!agents.some((agent) => agent.id === id && agent.available)}>{name}{agents.some((agent) => agent.id === id && agent.available) ? '' : ' · 未安装'}</option>)}
          </select><label className="automation-yolo-option"><input type="checkbox" checked={draft.yolo} onChange={(event) => update('yolo', event.target.checked)} />Yolo</label></div></div>
          <label><span>工作区方式</span><select aria-label="工作区" value={draft.workspace_mode} onChange={(event) => { const mode = event.target.value as WorkspaceMode; setDraft((current) => ({ ...current, workspace_mode: mode, cleanup_worktree: mode === 'new_worktree' ? true : current.cleanup_worktree })); }}>
            <option value="new_worktree">每次新建 Worktree</option><option value="new_branch">在现有工作区新建分支</option><option value="existing">使用现有工作区</option><option value="temporary">每次新建临时目录（TMP）</option>
          </select></label>
          <label><span>{draft.workspace_mode === 'temporary' ? '关联项目工作区' : draft.workspace_mode === 'new_worktree' ? '参考工作区' : '执行工作区'}</span><select aria-label={draft.workspace_mode === 'temporary' ? '关联项目工作区' : draft.workspace_mode === 'new_worktree' ? '参考工作区' : '执行工作区'} required value={draft.workspace_path} onChange={(event) => update('workspace_path', event.target.value)}>
            <option value="" disabled>选择工作区</option>
            {project.worktrees.map((worktree) => <option key={worktree.path} value={worktree.path}>{worktree.branch || 'detached'} · {worktree.path.split('/').pop()}</option>)}
          </select></label>
          {draft.workspace_mode === 'temporary' ? <p className="automation-hint">每次运行会在系统临时目录中创建空工作区，结束后自动删除；该任务的 Agent 会话不会出现在项目的普通会话列表中。</p> : null}
          {draft.workspace_mode !== 'existing' && draft.workspace_mode !== 'temporary' ? <label><span>基础分支</span><input aria-label="基础分支" required list="automation-branches" value={draft.base_branch} onChange={(event) => update('base_branch', event.target.value)} /><datalist id="automation-branches">{[...new Set(project.worktrees.map((worktree) => worktree.branch).filter(Boolean))].map((branch) => <option key={branch} value={branch} />)}</datalist></label> : null}
          {draft.workspace_mode === 'new_worktree' ? <p className="automation-hint">任务结束时会强制清理 Worktree，丢弃其中的未提交改动；自动创建的分支仍会保留。</p> : null}
          {draft.workspace_mode === 'new_branch' ? <p className="automation-hint">执行时会创建并切换到新分支；工作区须无未提交修改。</p> : null}
          <div className="automation-settings-divider" />
          <div className="automation-schedule-heading"><h3>{manual ? <MousePointerClick /> : <CalendarClock />}{manual ? '手动执行' : '运行计划'}</h3><label className="automation-checkbox"><input type="checkbox" checked={draft.max_concurrent_runs === 1} onChange={(event) => { if (event.target.checked) { if (draft.max_concurrent_runs > 1) setLastConcurrentRuns(draft.max_concurrent_runs); update('max_concurrent_runs', 1); } else update('max_concurrent_runs', lastConcurrentRuns); }} />禁止重叠执行</label></div>
          {draft.max_concurrent_runs > 1 ? <label><span>最大同时执行数</span><input aria-label="最大同时执行数" type="number" required min="2" max="10" step="1" value={draft.max_concurrent_runs} onChange={(event) => { const value = Number(event.target.value); setLastConcurrentRuns(value); update('max_concurrent_runs', value); }} /><small>达到上限时跳过本次触发，不排队。</small></label> : null}
          {!manual ? <>
          <label><span>频率</span><select aria-label="运行计划" value={schedule.cadence} onChange={(event) => setSchedule((current) => ({ ...current, cadence: event.target.value as Cadence }))}>
            <option value="interval">固定间隔</option><option value="hourly">每小时</option><option value="daily">每天</option><option value="weekdays">工作日</option><option value="weekly">每周</option><option value="custom">自定义 Cron</option>
          </select></label>
          {schedule.cadence === 'interval' ? <div className="automation-interval-fields">
            <label><span>间隔</span><input aria-label="运行间隔" type="number" required min="1" max={Math.floor(2678400 / intervalUnit)} step="1" placeholder="填写间隔" value={intervalAmount} onChange={(event) => setIntervalAmount(event.target.value)} /></label>
            <label><span>单位</span><select aria-label="间隔单位" value={intervalUnit} onChange={(event) => setIntervalUnit(Number(event.target.value))}><option value={1}>秒</option><option value={60}>分钟</option><option value={3600}>小时</option></select></label>
          </div> : schedule.cadence === 'custom' ? <label><span>Cron 表达式</span><input aria-label="Cron 表达式" required value={customCron} onChange={(event) => setCustomCron(event.target.value)} placeholder="0 9 * * 1-5" /><small>分 时 日 月 星期 · 支持 *、列表、范围、步长</small></label>
            : schedule.cadence === 'hourly' ? <label><span>每小时的第几分钟</span><input aria-label="每小时的第几分钟" type="number" required min="0" max="59" value={schedule.minute} onChange={(event) => setSchedule((current) => ({ ...current, minute: event.target.value }))} /></label>
              : <label><span>时间</span><input aria-label="时间" type="time" required value={schedule.time} onChange={(event) => setSchedule((current) => ({ ...current, time: event.target.value }))} /></label>}
          {schedule.cadence === 'weekly' ? <label><span>星期</span><select aria-label="星期" value={schedule.weekday} onChange={(event) => setSchedule((current) => ({ ...current, weekday: event.target.value }))}>{['日', '一', '二', '三', '四', '五', '六'].map((day, index) => <option key={day} value={index}>星期{day}</option>)}</select></label> : null}
          <p className="automation-hint">{schedule.cadence === 'interval' ? '启用后等待首个间隔，再按间隔触发。' : `按本机时区运行${timezone ? `：${timezone}` : ''}`}</p>
          </> : <p className="automation-hint">点击运行后填写变量，即可执行任务。</p>}
          <div className="automation-settings-divider" />
          <p className="automation-hint">{draft.max_concurrent_runs === 1 ? '上次执行未结束时跳过本次触发。' : `最多同时执行 ${draft.max_concurrent_runs} 个任务；达到上限时跳过本次触发。`}</p>
          <details open={!!task?.precheck_command}><summary><Terminal />执行前检查<span>可选</span></summary>
            <label><span>检查命令</span><textarea aria-label="检查命令" rows={3} value={draft.precheck_command} onChange={(event) => update('precheck_command', event.target.value)} placeholder="退出码为 0 时继续执行" /></label>
            <label><span>超时时间</span><select aria-label="超时时间" value={draft.precheck_timeout_seconds} onChange={(event) => update('precheck_timeout_seconds', Number(event.target.value))}>{[30, 60, 120, 300, 600].map((seconds) => <option key={seconds} value={seconds}>{seconds} 秒</option>)}</select></label>
          </details>
          <div className="automation-settings-divider" />
          <label><span>失败提醒</span><select aria-label="失败提醒" value={draft.failure_notification ?? ''} disabled={busy} onChange={event => update('failure_notification', event.target.value === 'feishu' || event.target.value === 'wechat' ? event.target.value : null)}>
            <option value="">不提醒</option>
            {(['feishu', 'wechat'] as const).map(provider => <option key={provider} value={provider} disabled={!availableBots.includes(provider)}>{provider === 'feishu' ? '飞书' : '微信'} Bot{botStatus === 'ready' && !availableBots.includes(provider) ? ' · 当前环境未配置' : ''}</option>)}
          </select></label>
          <p className="automation-hint">{botStatus === 'loading' ? '正在检查 IM Bot 配置…' : botStatus === 'error' ? '无法读取 IM Bot 配置，已有提醒设置仍会保留。' : draft.failure_notification && !availableBots.includes(draft.failure_notification) ? `当前环境未配置${draft.failure_notification === 'feishu' ? '飞书' : '微信'} Bot，已保留提醒设置；配置完成后，后续执行失败时会发送提醒。` : !availableBots.length ? '在 Settings → IM 配置飞书或微信 Bot 后，可选择失败提醒。' : '任务执行失败时通过所选 Bot 提醒；跳过和中断不提醒。'}</p>
        </aside>
      </div>
      {error ? <div className="automation-error" role="alert">{error}</div> : null}
      <footer>{!manual ? <label className="automation-checkbox"><input type="checkbox" checked={draft.enabled} onChange={(event) => update('enabled', event.target.checked)} />启用自动化</label> : <span />}<div><button type="button" disabled={busy} onClick={onClose}>取消</button><button className="automation-primary" type="submit" disabled={busy || !draft.workspace_path || !agents.some((agent) => agent.id === draft.agent && agent.available)}>{busy ? <LoaderCircle className="automation-spin" /> : null}{busy ? '保存中…' : task ? '保存更改' : manual ? '创建手动任务' : '创建自动化'}</button></div></footer>
    </form>
  </dialog>, document.body);
}
