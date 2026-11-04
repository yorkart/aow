import { t, getLanguage } from '../../lib/i18n';
import type { AutomationAgent, RunStatus } from './types';
export const agentNames: Record<AutomationAgent, string> = { codex: 'Codex', traecli: 'TraeCode CLI', claude: 'Claude Code', hermes: 'Hermes', pi: 'Pi' };
export function executionPermission(agent: AutomationAgent, yolo: boolean) {
  if (agent === 'pi') return yolo ? t("automations.trust_project_configuration") : t("automations.ignore_project_configuration");
  return yolo ? 'Yolo / Full Access' : t("automations.standard_permissions");
}
export { workspaceNames } from '../workspaces/types';
export const runNames: Record<RunStatus, string> = { preparing: t("automations.preparing"), running: t("aow.running"), completed: t("aow.completed"), failed: t("aow.failed"), skipped: t("aow.skipped"), interrupted: t("aow.interrupted") };
export const errorMessage = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
export function dateTime(value: string | null) {
  return value ? new Date(value).toLocaleString(getLanguage(), { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }) : '—';
}
export function duration(ms: number | null) {
  if (ms === null) return '—';
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return t("aow.seconds", { count: (ms / 1000).toFixed(1), value0: (ms / 1000).toFixed(1) });
  const totalSeconds = Math.floor(ms / 1000);
  const seconds = totalSeconds % 60;
  const totalMinutes = Math.floor(totalSeconds / 60);
  const minutes = totalMinutes % 60;
  const hours = Math.floor(totalMinutes / 60);
  return hours > 0
    ? t("automations.h_m_s", { value0: hours, value1: minutes, value2: seconds })
    : t("automations.m_s", { value0: minutes, value1: seconds });
}
export function scheduleName(cron: string, seconds?: number | null) {
  if (seconds) {
    const unit = seconds % 3600 === 0 ? 3600 : seconds % 60 === 0 ? 60 : 1;
    return t("automations.every", { value0: seconds / unit, value1: unit === 3600 ? t("automations.hours") : unit === 60 ? t("automations.minutes") : t("automations.seconds") });
  }
  const [minute, hour, day, month, weekday] = cron.trim().split(/\s+/);
  if (day === '*' && month === '*' && /^\d+$/.test(minute)) {
    if (hour === '*' && weekday === '*') return t("automations.hourly_at", { value0: minute.padStart(2, '0') });
    if (/^\d+$/.test(hour)) {
      const time = `${hour.padStart(2, '0')}:${minute.padStart(2, '0')}`;
      if (weekday === '*') return t("automations.daily_at", { value0: time });
      if (weekday === '1-5') return t("automations.weekdays_at", { value0: time });
      if (/^[0-7]$/.test(weekday)) return t("automations.every_at", { value0: new Intl.DateTimeFormat(getLanguage(), { weekday: 'long', timeZone: 'UTC' }).format(new Date(Date.UTC(2026, 0, 4 + Number(weekday) % 7))), value1: time });
    }
  }
  return cron;
}
