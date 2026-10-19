// conversation_view.rs::render_auth_required_state: render only an explicit AuthRequired state.
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
  const title = `${pending ? '正在登录' : '登录'} ${text(connection.agent_info?.title) || text(connection.agent_info?.name) || connection.agent_id}`;
  const description = pending ? '请完成 Agent 提供的认证步骤…' : !methods.length ? '此 Agent 没有提供当前支持的登录方式。' : methods.length > 1 ? '选择一种登录方式以继续。' : composer ? '登录后可继续对话，未发送的草稿会保留。' : undefined;
  const actions = !pending && !!methods.length && [...methods].reverse().map(method => <button type="button" key={text(method.methodId) || text(method.id)} title={text(method.description)} onClick={() => onAuthenticate(text(method.methodId) || text(method.id))}>{text(method.name) || text(method.methodId) || text(method.id)}</button>);
  const details = requests.length || error || sync.error || externalError ? <>
    {requests.map(request => <PermissionRequest key={request.id} permission={request} busy={answering} onAnswer={response => {
      setAnswering(true); setError('');
      void acpApi.answerConnection(connection.id, request.id, response).catch(reason => setError(failure(reason))).finally(() => setAnswering(false));
    }} />)}
    {(error || sync.error || externalError) && <p className="acp-error" role="alert">{error || sync.error || externalError}</p>}
  </> : undefined;
  return composer ? <ComposerStatus label="Agent 登录" icon={icon} title={title} description={description} actions={actions}>{details}</ComposerStatus>
    : <section className="acp-auth-callout" aria-label="Agent 登录"><div>{icon}<strong>{title}</strong></div>{description && <p role={pending ? 'status' : undefined}>{description}</p>}{actions && <div className="acp-auth-actions">{actions}</div>}{details}</section>;
}
