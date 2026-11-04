import { t } from '../../lib/i18n';
import { LoaderCircle, LockKeyhole, ShieldCheck, WifiOff } from 'lucide-react';
import { ComposerStatus } from '../../components/conversation/ComposerStatus';
import type { ControlState } from './sessionControl';

export function ControlStatus({ state, takeover }: ControlState & { takeover: () => void }) {
  if (state === 'claimed') return null;
  return <ComposerStatus
    icon={state === 'observing' ? <LockKeyhole /> : state === 'connecting' ? <LoaderCircle className="acp-spinner" /> : <WifiOff />}
    title={state === 'observing' ? t("acp-control.take_control_to_continue_the_conversation") : state === 'connecting' ? t("acp-control.acquiring_session_control") : t("acp-control.control_connection_lost_reconnecting")}
    description={state === 'observing' ? t("acp-control.this_page_is_read_only_take_control_to_type") : t("acp-control.input_resumes_when_the_connection_is_ready_unsent_drafts")}
    actions={state === 'observing' && <button type="button" onClick={takeover}><ShieldCheck size={14} />{t("acp-control.take_control")}</button>}
  />;
}
