import { useEffect, useRef, useState } from 'react';
import { ArrowLeft, ExternalLink, RefreshCw, Trash2 } from 'lucide-react';
import { authenticationStatus, cancelAuthentication, checkConnector, connectorCatalog, enableConnector, getConnector, removeConnector, saveConnector, startAuthentication, submitAuthentication, verifyConnector } from './api';
import { ConnectorBadge } from './ConnectorsPage';
import { authNames, executorNames, providerNames, type AuthMethod, type AuthSession, type Catalog, type CheckResult, type Connector, type ConnectorFields } from './types';

const empty: ConnectorFields = { name: '', provider: 'openai', executor: 'codex', auth_method: 'local', account_note: '' };
const fieldsOf = (row: Connector): ConnectorFields => ({ name: row.name, provider: row.provider, executor: row.executor, auth_method: row.auth_method, account_note: row.account_note });
const activeAuthentication = (session: AuthSession) => ['starting', 'waiting', 'completing'].includes(session.status);

export function ConnectorForm({ org, id, onBack, onDirtyChange }: { org: string; id?: string; onBack: () => void; onDirtyChange: (dirty: boolean) => void }) {
  const [catalog, setCatalog] = useState<Catalog>();
  const [record, setRecord] = useState<Connector>();
  const [fields, setFields] = useState<ConnectorFields>(empty);
  const [baseline, setBaseline] = useState(JSON.stringify(empty));
  const [apiKey, setApiKey] = useState('');
  const [session, setSession] = useState<AuthSession>();
  const [code, setCode] = useState('');
  const [preview, setPreview] = useState<CheckResult>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [retry, setRetry] = useState(0);
  const lock = useRef(false);
  const authentication = useRef<string | undefined>(undefined);
  const dirty = JSON.stringify(fields) !== baseline || Boolean(apiKey) || Boolean(session && !['cancelled', 'expired', 'error'].includes(session.status));
  useEffect(() => {
    const abort = new AbortController();
    void Promise.all([connectorCatalog(org, abort.signal), id ? getConnector(org, id, abort.signal) : Promise.resolve(undefined)])
      .then(([catalog, record]) => { if (abort.signal.aborted) return; setCatalog(catalog); setRecord(record); const fields = record ? fieldsOf(record) : empty; setFields(fields); setBaseline(JSON.stringify(fields)); })
      .catch((error: unknown) => { if (!abort.signal.aborted) setError(error instanceof Error ? error.message : '无法读取连接配置。'); });
    return () => abort.abort();
  }, [org, id, retry]);
  useEffect(() => { onDirtyChange(dirty); return () => onDirtyChange(false); }, [dirty, onDirtyChange]);
  useEffect(() => {
    if (!dirty) return;
    const unload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ''; };
    window.addEventListener('beforeunload', unload); return () => window.removeEventListener('beforeunload', unload);
  }, [dirty]);
  useEffect(() => () => { if (authentication.current) void cancelAuthentication(org, authentication.current).catch(() => {}); }, [org]);
  useEffect(() => {
    if (!session?.id || !activeAuthentication(session)) return;
    let active = true; let timer: number;
    const poll = async () => {
      try {
        const next = await authenticationStatus(org, session.id);
        if (!active) return;
        setSession(next);
        if (activeAuthentication(next)) timer = window.setTimeout(() => { void poll(); }, 1000);
      } catch (error) { if (active) setError(error instanceof Error ? error.message : '无法读取授权进度。'); }
    };
    timer = window.setTimeout(() => { void poll(); }, 300);
    return () => { active = false; window.clearTimeout(timer); };
  }, [org, session?.id]);
  const perform = async (operation: () => Promise<void>) => {
    if (lock.current) return; lock.current = true; setBusy(true); setError('');
    try { await operation(); } catch (error) { setError(error instanceof Error ? error.message : '操作失败，请重试。'); }
    finally { lock.current = false; setBusy(false); }
  };
  const cancel = () => {
    if (authentication.current) void cancelAuthentication(org, authentication.current).catch(() => {});
    authentication.current = undefined; setSession(undefined); setCode('');
  };
  const changeAuthentication = (next: ConnectorFields) => { cancel(); setApiKey(''); setPreview(undefined); setFields(next); setError(''); };
  const leave = () => { if (!busy && (!dirty || window.confirm('放弃尚未保存的连接配置？'))) onBack(); };
  const extra = () => ({ connector_id: record?.id, ...(apiKey ? { api_key: apiKey } : {}), ...(session?.status === 'complete' ? { auth_session_id: session.id } : {}) });
  const save = (connect: boolean) => { void perform(async () => {
    await saveConnector(org, fields, { id: record?.id, revision: record?.revision, ...extra(), connect });
    authentication.current = undefined; setApiKey(''); onDirtyChange(false); onBack();
  }); };
  if (!catalog || id && !record) return <section className="connectors-page"><button className="connectors-back" type="button" onClick={onBack}><ArrowLeft size={15} />返回本地 Connector</button>{error ? <p className="connectors-error" role="alert">{error}<button type="button" onClick={() => { setError(''); setRetry(value => value + 1); }}>重试</button></p> : <p role="status">正在读取连接配置…</p>}</section>;
  const provider = catalog.providers.find(item => item.id === fields.provider);
  const executor = catalog.executors.find(item => item.id === fields.executor);
  const sameAuthentication = record && fields.provider === record.provider && fields.executor === record.executor && fields.auth_method === record.auth_method;
  const reusable = Boolean(sameAuthentication && record.credential_configured);
  const authReady = fields.auth_method === 'local' || fields.auth_method === 'api_key' && Boolean(apiKey || reusable) || fields.auth_method === 'independent' && Boolean(session?.status === 'complete' || reusable);
  return <section className="connectors-page"><button className="connectors-back" type="button" disabled={busy} onClick={leave}><ArrowLeft size={15} />返回本地 Connector</button>
    <header className="connectors-heading"><div><h1>{record ? '编辑 Connector' : '新增 Connector'}</h1><p>配置模型服务、执行器与认证身份。</p></div>{record && <ConnectorBadge status={record.status} />}</header>
    <form className="connector-form" onSubmit={event => { event.preventDefault(); save(false); }}>
      <label>连接名称<input autoFocus required maxLength={100} disabled={busy} placeholder="例如：Codex · 本机账号" value={fields.name} onChange={event => setFields(current => ({ ...current, name: event.target.value }))} /></label>
      <div className="connector-form-columns"><label>Provider<select aria-label="Provider" value={fields.provider} disabled={busy} onChange={event => {
        const provider = catalog.providers.find(item => item.id === event.target.value)!;
        changeAuthentication({ ...fields, provider: provider.id, executor: provider.executors[0], auth_method: provider.auth_methods[0] });
      }}>{catalog.providers.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}{!provider && <option value={fields.provider}>{providerNames[fields.provider] ?? fields.provider}</option>}</select></label>
        <label>执行器<select aria-label="执行器" disabled={busy || (provider?.executors.length ?? 0) <= 1} value={fields.executor} onChange={event => changeAuthentication({ ...fields, executor: event.target.value })}>{(provider?.executors ?? [fields.executor]).map(id => <option key={id} value={id}>{executorNames[id] ?? id}{catalog.executors.find(item => item.id === id)?.installed ? '' : '（未安装）'}</option>)}</select></label></div>
      {!executor?.installed && <p className="connectors-note">本机尚未找到 {executorNames[fields.executor]}。可先保存配置，安装执行器后再认证。</p>}
      <label>账号备注<input maxLength={500} disabled={busy} value={fields.account_note} placeholder="便于识别账号用途" onChange={event => setFields(current => ({ ...current, account_note: event.target.value }))} /></label>
      <fieldset className="connector-auth-methods"><legend>认证方式</legend>{(provider?.auth_methods ?? [fields.auth_method]).map(method => <button key={method} type="button" aria-pressed={fields.auth_method === method} disabled={busy} onClick={() => { if (fields.auth_method !== method) changeAuthentication({ ...fields, auth_method: method as AuthMethod }); }}>{authNames[method]}</button>)}</fieldset>
      <section className="connector-auth-region" aria-label="认证配置">
        {fields.auth_method === 'local' ? <><h2>复用本地登录</h2><p>使用执行器所在机器已登录的身份。每次执行前，认证所需内容会重新复制到隔离环境。</p><button type="button" disabled={busy} onClick={() => { void perform(async () => setPreview(await verifyConnector(org, fields, extra()))); }}><RefreshCw size={15} />检测本地身份</button></>
          : fields.auth_method === 'api_key' ? <><h2>API Key</h2><p>密钥经 Provider 验证后加密保存，编辑时不回显。</p><label>密钥<input type="password" autoComplete="off" disabled={busy} value={apiKey} placeholder={reusable ? '已配置，留空保留已有密钥' : '输入 Provider API Key'} onChange={event => { setApiKey(event.target.value); setPreview(undefined); }} /></label>{reusable && <small className="connectors-note">已配置密钥</small>}<button type="button" disabled={busy || !apiKey && !reusable} onClick={() => { void perform(async () => setPreview(await verifyConnector(org, fields, extra()))); }}>验证密钥与执行器</button></>
          : <><h2>独立登录</h2><p>{fields.executor === 'codex' ? '由 Codex 启动设备码登录，在官方页面完成认证。' : '由 Claude Code 启动浏览器授权，完成后回填官方页面提供的授权码。'}</p>{reusable && !session && <p className="connectors-note">已保存独立登录身份，可重新授权替换。</p>}
            <button type="button" disabled={busy || Boolean(session && activeAuthentication(session))} onClick={() => { void perform(async () => { cancel(); const next = await startAuthentication(org, fields); authentication.current = next.id; setSession(next); }); }}>开始独立登录</button>
            {session && <div className="connector-login"><p role="status">{session.message}</p>{session.verification_uri && <a href={session.verification_uri} target="_blank" rel="noopener noreferrer">前往官方授权页面<ExternalLink size={14} /></a>}{session.user_code && <p>设备码 <code className="connector-device-code">{session.user_code}</code></p>}{fields.executor === 'claude' && session.status === 'waiting' && <><label>授权码<input type="password" autoComplete="off" disabled={busy} value={code} onChange={event => setCode(event.target.value)} /></label><button type="button" disabled={busy || !code.trim()} onClick={() => { void perform(async () => { setSession(await submitAuthentication(org, session.id, code.trim())); setCode(''); }); }}>提交授权码</button></>}{session.account_summary && <p>{session.account_summary}</p>}{activeAuthentication(session) && <button type="button" disabled={busy} onClick={() => { void perform(async () => { await cancelAuthentication(org, session.id); authentication.current = undefined; setSession({ ...session, status: 'cancelled', message: '授权已取消。', verification_uri: null, user_code: null }); }); }}>取消授权</button>}</div>}
          </>}
        {preview && <div className={`connector-check connector-check-${preview.status}`} role="status"><ConnectorBadge status={preview.status} /><p>{preview.message}</p>{preview.summary && <strong>{preview.summary}</strong>}</div>}
        {!preview && record?.account_summary && sameAuthentication && <p className="connector-account">账号摘要：{record.account_summary}</p>}
      </section>
      {error && <p className="connectors-error" role="alert">{error}</p>}
      <footer className="connector-form-actions"><button type="button" disabled={busy} onClick={leave}>取消</button><button type="submit" disabled={busy || !fields.name.trim()}>保存配置</button><button className="orbit-primary" type="button" disabled={busy || !fields.name.trim() || !authReady} onClick={() => save(true)}>{busy ? '处理中…' : '保存并连接'}</button></footer>
    </form>
    {record && <section className="connector-management"><h2>连接状态</h2><p>{record.status_message}</p><div className="connector-management-actions"><button type="button" disabled={busy || dirty || !record.enabled} onClick={() => { void perform(async () => setRecord(await checkConnector(org, record.id))); }}>检查连接</button><button type="button" disabled={busy || dirty} onClick={() => { void perform(async () => setRecord(await enableConnector(org, record))); }}>{record.enabled ? '停用连接' : '启用连接'}</button><button className="connector-delete" type="button" disabled={busy || record.agent_count > 0} title={record.agent_count ? '请先解除 Agent 引用' : undefined} onClick={() => { if (window.confirm(`删除连接「${record.name}」及其保存的凭据？`)) void perform(async () => { await removeConnector(org, record); onBack(); }); }}><Trash2 size={14} />删除连接</button></div><h2>引用此连接的 Agent</h2>{record.agents.length ? <ul>{record.agents.map(agent => <li key={agent.agent_id}>{agent.agent_id}</li>)}</ul> : <p className="connectors-note">还没有 Agent 引用此连接。</p>}</section>}
  </section>;
}
