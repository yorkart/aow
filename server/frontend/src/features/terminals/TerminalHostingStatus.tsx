import { t } from '../../lib/i18n';
import { useEffect, useState } from 'react';
import { AlertTriangle, CircleCheck } from 'lucide-react';
import { aowApi } from '../../aow/aowApi';
import { useAowTabNavigation } from '../../aow/AowTabEntry';
import { tabTargetUrl, type TabTarget } from '../../aow/tabRoutes';
import type { TerminalHosting } from './types';
import './terminal-hosting.css';

const hostingPhases: Record<TerminalHosting['phase'], string> = {
  waiting: t("terminals.waiting_for_this_turn_to_finish"), reviewing: t("terminals.checking"), collecting: t("terminals.reading_conclusion"), submitting: t("terminals.submitting_feedback"), failed: t("automations.paused"),
  completed: t("terminals.review_passed"), limit_reached: t("terminals.input_limit_reached"),
};

export function hostingStatus(hosting: TerminalHosting) {
  const state = hosting.phase === 'completed' ? t("terminals.autopilot_finished") : hosting.phase === 'limit_reached' ? t("terminals.autopilot_stopped") : t("terminals.autopilot_active");
  return t("terminals.inputs", { value0: state, value1: hosting.task_name, value2: hostingPhases[hosting.phase], value3: hosting.input_count, value4: hosting.max_inputs });
}

export function TerminalHostingDetails({ hosting }: { hosting: TerminalHosting }) {
  const navigate = useAowTabNavigation();
  const [target, setTarget] = useState<TabTarget>();
  const [error, setError] = useState('');
  const completed = hosting.phase === 'completed';
  const message = hosting.error || (hosting.phase === 'limit_reached' ? t("terminals.take_control_to_address_the_remaining_review_findings") : '');
  const show = Boolean(message) || completed;
  useEffect(() => {
    let active = true;
    setTarget(undefined); setError('');
    if (!show || !hosting.run_id) return;
    void aowApi.projects().then(projects => {
      const worktree = projects.flatMap(project => project.worktrees).find(worktree => worktree.path === hosting.workspace_root);
      if (!worktree) throw new Error(t("terminals.the_workspace_for_this_run_has_been_removed"));
      if (active) setTarget({ type: 'automation', workspace: worktree.id, taskId: hosting.task_id, runId: hosting.run_id!, view: 'runs' });
    }).catch(reason => { if (active) setError(reason instanceof Error ? reason.message : String(reason)); });
    return () => { active = false; };
  }, [show, hosting.task_id, hosting.run_id, hosting.workspace_root]);
  if (!show) return null;
  return <div className={`terminal-hosting-details${completed ? ' completed' : ''}`} role={completed && !error ? 'status' : 'alert'}>
    {completed ? <CircleCheck /> : <AlertTriangle />}{(message || error) && <span>{message}{error ? `${message ? ' · ' : ''}${error}` : ''}</span>}
    {target && <a href={tabTargetUrl(target)} onClick={event => {
      event.stopPropagation();
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      event.preventDefault();
      void navigate(target).catch(reason => setError(reason instanceof Error ? reason.message : String(reason)));
    }}>{t("terminals.view_this_run")}</a>}
  </div>;
}
