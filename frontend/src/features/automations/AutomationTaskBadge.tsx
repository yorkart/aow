import { t } from '../../lib/i18n';
import type { AutomationTask } from './types';

export function AutomationTaskBadge({ task }: { task: AutomationTask }) {
  const state = task.scheduler_error ? 'failed' : task.is_running ? 'running' : task.enabled ? 'enabled' : 'paused';
  return <span className={`automation-badge ${state}`}><i />{task.scheduler_error ? t("automations.configuration_error") : task.is_running ? t("aow.running") : task.kind === 'manual' ? t("automations.manual") : task.enabled ? t("automations.enabled") : t("automations.paused")}</span>;
}
