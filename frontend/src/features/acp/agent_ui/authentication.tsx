// conversation_view.rs::render_auth_required_state: render only an explicit AuthRequired state.
import { LoaderCircle, LogIn } from 'lucide-react';
import { useState } from 'react';
import { useRequestsSync } from '../sync/hooks';
import { acpApi } from '../api';
import { failure, object, text, type ConnectionInfo } from '../types';
import { PermissionRequest } from './conversation_view/elicitation';

export function Authentication({ connection, pending, onAuthenticate }: { connection: ConnectionInfo; pending: boolean; onAuthenticate: (method: string) => void }) {
  const sync = useRequestsSync(pending ? connection.id : undefined);
  const requests = (sync.data ?? []).filter(item => !item.request.sessionId && !object(item.request.scope).sessionId);
  const [answering, setAnswering] = useState(false);
  const [error, setError] = useState('');
  const methods = (connection.auth_methods ?? []).filter(method => !method.type || method.type === 'agent');
  return <section className="acp-auth-callout" aria-label="Agent 登录">
    <div>{pending ? <LoaderCircle className="acp-spinner" size={16} /> : <LogIn size={16} />}<strong>{pending ? '正在登录' : '登录'} {text(connection.agent_info?.title) || text(connection.agent_info?.name) || connection.agent_id}</strong></div>
    {pending ? <p role="status">请完成 Agent 提供的认证步骤…</p> : <>
      {methods.length > 1 && <p>选择一种登录方式以继续。</p>}
      {!methods.length && <p>此 Agent 没有提供当前支持的登录方式。</p>}
      <div className="acp-auth-actions">{[...methods].reverse().map(method => <button type="button" key={text(method.methodId) || text(method.id)} title={text(method.description)} onClick={() => onAuthenticate(text(method.methodId) || text(method.id))}>{text(method.name) || text(method.methodId) || text(method.id)}</button>)}</div>
    </>}
    {requests.map(request => <PermissionRequest key={request.id} permission={request} busy={answering} onAnswer={response => {
      setAnswering(true); setError('');
      void acpApi.answerConnection(connection.id, request.id, response).catch(reason => setError(failure(reason))).finally(() => setAnswering(false));
    }} />)}
    {(error || sync.error) && <p className="acp-error" role="alert">{error || sync.error}</p>}
  </section>;
}
