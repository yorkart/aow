import { t, getLanguage } from '../../lib/i18n';
export function parsePullRequestNumber(value?: string): number | undefined {
  if (!value || !/^\d+$/.test(value)) return undefined;
  const number = Number(value);
  return Number.isSafeInteger(number) && number > 0 ? number : undefined;
}

export function stateClass(value: string): 'passed' | 'failed' | 'pending' | 'neutral' {
  const state = value.toLowerCase();
  if (['passed', 'succeeded', 'success', 'approved', 'all_passed', 'resolved'].includes(state)) return 'passed';
  if (['failed', 'failure', 'disapproved', 'some_failed', 'timed_out', 'canceled', 'cancelled', 'operation_required', 'action_required', 'startup_failure', 'changes_requested', 'error', 'rejected'].includes(state)) return 'failed';
  if (['pending', 'running', 'in_progress', 'queued', 'warning', 'not_passed', 'unreviewed', 'review_required', 'expected'].includes(state)) return 'pending';
  return 'neutral';
}

export function stateLabel(value: string): string {
  const labels: Record<string, string> = {
    open: t("components.in_progress"), closed: t("pr.closed"), merged: t("pr.merged"), draft: t("pr.draft"),
    passed: t("pr.passed"), succeeded: t("pr.passed"), success: t("pr.passed"), approved: t("pr.passed"), all_passed: t("pr.all_passed"),
    failed: t("pr.failed"), failure: t("pr.failed"), disapproved: t("pr.changes_requested"), rejected: t("pr.changes_requested"), some_failed: t("pr.some_failed"),
    pending: t("aow.waiting"), running: t("pr.running"), in_progress: t("pr.running"), queued: t("aow.queued"),
    warning: t("pr.warnings"), not_passed: t("pr.pending_approval"), unreviewed: t("pr.awaiting_review"),
    timed_out: t("pr.timed_out"), canceled: t("acp.canceled"), cancelled: t("acp.canceled"), operation_required: t("pr.action_required"),
    completed: t("aow.completed"), skipped: t("aow.skipped"), neutral: t("pr.no_conclusion"), no_checks: t("pr.no_checks"),
    changes_requested: t("pr.changes_requested"), review_required: t("pr.awaiting_review"), action_required: t("pr.action_required"), startup_failure: t("pr.failed_to_start"), error: t("operations.error"), expected: t("aow.waiting"), comment: t("pr.general_discussion"),
    unknown: t("pr.unknown_status"), resolved: t("pr.resolved"),
  };
  return labels[value.toLowerCase()] || value || t("pr.unknown_status");
}

export function safeExternalUrl(value?: string | null): string | undefined {
  if (!value) return undefined;
  try {
    const url = new URL(value);
    return ['https:', 'http:'].includes(url.protocol) ? url.href : undefined;
  } catch { return undefined; }
}

export function formatPrTime(value?: string | null): string {
  if (!value) return t("pr.not_provided");
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return t("pr.not_provided");
  return new Intl.DateTimeFormat(getLanguage(), { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false }).format(date);
}

export function mergeCheckLabel(name: string): string {
  const labels: Record<string, string> = {
    merge_state: t("pr.merge_status"),
    checkNoConflict: t("pr.no_branch_conflicts"), checkReviewPassed: t("pr.code_review"), checkCheckRun: t("pr.ci_checks"),
    checkCheckRuns: t("pr.ci_checks"), checkCheckRunPassed: t("pr.ci_checks"), checkAllThreadsResolved: t("pr.discussions_resolved"),
    checkNoUnresolvedThreads: t("pr.discussions_resolved"), checkNotDraft: t("pr.ready_for_review"),
    checkSourceBranchExists: t("pr.source_branch_available"), checkTargetBranchExists: t("pr.target_branch_available"),
    checkFastForward: t("pr.fast_forward_merge_possible"), checkPullRequestOpen: t("pr.pr_is_open"),
  };
  return labels[name] || name;
}
