import { t } from '../../lib/i18n';
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
      if (!project) throw new Error(t("terminals.current_workspace_is_not_linked_to_a_project"));
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
    <button ref={button} type="button" className="terminal-hosting-button" title={t("terminals.choose_a_manual_task_and_enable_autopilot")} aria-label={t("navigation.autopilot")}
      aria-haspopup="dialog" aria-expanded={Boolean(position)} disabled={Boolean(pane.hosting) || pane.status !== 'running' || busy || creating}
      onClick={event => {
        event.stopPropagation();
        if (position) setPosition(undefined); else openMenu();
      }}><Bot />{t("navigation.autopilot")}</button>
    {position && createPortal(<div ref={popup} className="terminal-hosting-menu" style={position} role="dialog" aria-label={t("terminals.choose_autopilot_task")}
      onPointerDown={event => event.stopPropagation()} onClick={event => event.stopPropagation()}>
      <strong>{t("terminals.choose_autopilot_task")}</strong>
      <p className="terminal-hosting-limit-hint">{t("terminals.supports_only")}<button type="button" className="terminal-hosting-create" title={t("automations.create_manual_task")} disabled={!project || busy || creating} onClick={() => setCreating(true)}><strong>{t("terminals.dynamic_workspace")}</strong></button>{t("terminals.manual_tasks")}</p>
      <label className="terminal-hosting-limit">{t("terminals.maximum_automatic_inputs")}<input type="number" min="1" step="1" aria-label={t("terminals.maximum_automatic_inputs")} value={maxInputs} disabled={busy} onChange={event => setMaxInputs(event.target.value)} /></label>
      <p className="terminal-hosting-limit-hint">{t("terminals.at_the_limit_run_one_final_review_without_sending")}</p>
      <label className="terminal-hosting-run-on-enable"><input type="checkbox" checked={runOnEnable} disabled={busy} onChange={event => setRunOnEnable(event.target.checked)} />{t("terminals.run_immediately_when_autopilot_starts")}</label>
      <p className="terminal-hosting-limit-hint">{t("terminals.do_not_wait_for_the_current_task_to_finish")}</p>
      {!validLimit && <p className="terminal-hosting-menu-error" role="alert">{t("terminals.enter_a_valid_positive_integer")}</p>}
      <label className="terminal-hosting-search"><Search /><input autoFocus placeholder={t("terminals.search_manual_tasks")} aria-label={t("terminals.search_manual_tasks_2")} value={search} onChange={event => setSearch(event.target.value)} /></label>
      <div className="terminal-hosting-tasks">
        {loading ? <p role="status"><LoaderCircle className="spinning" />{t("automations.loading")}</p> : matches.map(task => <button key={task.id} type="button" disabled={busy || creating || !validLimit} onClick={() => void select(task)}>
          <span>{task.name}</span><small>{task.prompt.replace(/\s+/g, ' ').slice(0, 100)}</small>
        </button>)}
        {!loading && !matches.length && <p>{tasks.length ? t("terminals.no_matching_tasks") : t("terminals.no_tasks_available_click_dynamic_workspace_above_to_create")}</p>}
      </div>
      {busy && <p role="status">{t("terminals.starting_autopilot")}</p>}
      {creating && <p role="status">{t("terminals.opening_creation_dialog")}</p>}
      {error && <p className="terminal-hosting-menu-error" role="alert">{error}</p>}
    </div>, document.body)}
    {editor && <AutomationEditor kind="manual" initialWorkspaceMode="dynamic" project={editor.project} agents={editor.agents}
      onClose={() => { setEditor(undefined); requestAnimationFrame(() => button.current?.focus()); }}
      onSaved={() => { setEditor(undefined); openMenu(); }} />}
  </>;
}
