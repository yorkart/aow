import { t } from '../../../lib/i18n';
import { LoaderCircle, LogIn } from 'lucide-react';
import { useState } from 'react';
import { ComposerStatus } from '../../../components/conversation/ComposerStatus';
import { useRequestsSync } from '../sync/hooks';
import { acpApi } from '../api';
import { failure, object, text, type ConnectionInfo } from '../types';
import { PermissionRequest } from './conversation_view/elicitation';

export function Authentication({ connection, pending, onAuthenticate, composer = false, error: externalError }: { connection: ConnectionInfo; pending: boolean; onAuthenticate: (method: string) => void; composer?: boolean; error?: string }) {
  const sync = useRequestsSync(pending ? connection.id : undefined);
  const requests = (sync.data ?? []).filter(item => !item.request.sessionId && !object(item.request.scope).sessionId);
  const [answering, setAnswering] = useState(false);
  const [error, setError] = useState('');
  const methods = (connection.auth_methods ?? []).filter(method => !method.type || method.type === 'agent');
  const icon = pending ? <LoaderCircle className="acp-spinner" size={16} /> : <LogIn size={16} />;
  const title = `${pending ? t("common.signing_in") : t("auth.sign_in")} ${text(connection.agent_info?.title) || text(connection.agent_info?.name) || connection.agent_id}`;
  const description = pending ? t("acp.complete_the_authentication_steps_provided_by_the_agent") : !methods.length ? t("acp.this_agent_has_no_supported_login_methods") : methods.length > 1 ? t("acp.choose_a_login_method_to_continue") : composer ? t("acp.sign_in_to_continue_the_conversation_unsent_drafts_will") : undefined;
  const actions = !pending && !!methods.length && [...methods].reverse().map(method => <button type="button" key={text(method.methodId) || text(method.id)} title={text(method.description)} onClick={() => onAuthenticate(text(method.methodId) || text(method.id))}>{text(method.name) || text(method.methodId) || text(method.id)}</button>);
  const details = requests.length || error || sync.error || externalError ? <>
    {requests.map(request => <PermissionRequest key={request.id} permission={request} busy={answering} onAnswer={response => {
      setAnswering(true); setError('');
      void acpApi.answerConnection(connection.id, request.id, response).catch(reason => setError(failure(reason))).finally(() => setAnswering(false));
    }} />)}
    {(error || sync.error || externalError) && <p className="acp-error" role="alert">{error || sync.error || externalError}</p>}
  </> : undefined;
  return composer ? <ComposerStatus label={t("acp.agent_login")} icon={icon} title={title} description={description} actions={actions}>{details}</ComposerStatus>
    : <section className="acp-auth-callout" aria-label={t("acp.agent_login")}><div>{icon}<strong>{title}</strong></div>{description && <p role={pending ? 'status' : undefined}>{description}</p>}{actions && <div className="acp-auth-actions">{actions}</div>}{details}</section>;
}
