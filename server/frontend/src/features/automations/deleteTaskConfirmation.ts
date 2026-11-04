import { t } from '../../lib/i18n';
import type { ConfirmationOptions } from '../../components/ConfirmationDialog';
import type { AutomationTask } from './types';

export function deleteTaskConfirmation(task: AutomationTask): ConfirmationOptions {
  return {
    title: task.kind === 'manual' ? t("automations.delete_manual_task") : t("automations.delete_automation"),
    description: t("automations.delete_run_history_and_created_workspaces_will_be_retained", { value0: task.name, value1: task.kind === 'manual' ? '' : t("common.and_remove_its_schedule") }),
    confirmLabel: t("acp.delete"),
    danger: true,
  };
}
