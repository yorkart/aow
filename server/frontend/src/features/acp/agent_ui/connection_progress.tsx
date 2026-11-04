import { t } from '../../../lib/i18n';
import { useEffect, useState, type ReactNode } from 'react';
import { LoaderCircle } from 'lucide-react';
import { ComposerStatus } from '../../../components/conversation/ComposerStatus';
import type { ConnectionStatus } from '../types';

const phases: Record<string, string> = {
  restoring: t("acp.restoring_session"), ready: t("acp.action_completed"), failed: t("acp.action_failed"),
  queued: t("acp.waiting_for_another_agent_installation_to_finish"), configuration: t("acp.reading_configuration"), registry: t("acp.reading_acp_registry"),
  runtime: t("acp.checking_node_js_and_npm"), installing: t("acp.downloading_and_installing_agent_dependencies"),
  download: t("acp.downloading_agent"), extract: t("acp.extracting_agent"), starting: t("acp.starting_agent"), initializing: t("acp.establishing_acp_connection"),
};
export function ConnectionProgress({ status, message, composer = false, actions, children }: { status?: ConnectionStatus; message: string; composer?: boolean; actions?: ReactNode; children?: ReactNode }) {
  const [seconds, setSeconds] = useState(0);
  useEffect(() => { const started = Date.now(); const timer = window.setInterval(() => setSeconds(Math.floor((Date.now() - started) / 1000)), 1000); return () => window.clearInterval(timer); }, []);
  const title = status ? phases[status.phase] || message : message;
  const elapsed = t("acp.waited_seconds", { value0: Math.max(seconds, status?.elapsed_seconds ?? 0) });
  const details = <>
    {status?.detail && <p>{t("acp.network_connection_failed")}{status.detail}{t("acp.waiting_to_retry_configure_the_service_proxy_in_settings")}</p>}
    {status?.phase === 'installing' && !status.detail && <p>{t("acp.first_installation_downloads_dependencies_and_may_take_up_to")}</p>}
  </>;
  return composer ? <ComposerStatus icon={<LoaderCircle className="acp-spinner" />} title={title} description={<><span>{elapsed}</span>{details}</>} actions={actions}>{children}</ComposerStatus>
    : <div className="acp-status" role="status"><span>{title} · {elapsed}</span>{details}</div>;
}
