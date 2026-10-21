import { useEffect, useState } from 'react';
import { aowRequest } from '../../lib/aowRequest';
import { readServerEnvironment, serverEnvironmentFields, serverEnvironmentGroups, validateServerEnvironment, writeServerEnvironment } from './serverEnvironment';
import type { ServerEnvironmentValues } from './serverEnvironment';
import './server-environment.css';

interface ServerEnvironment {
  path: string;
  content: string;
  revision: string;
  exists: boolean;
  platform: string;
}

const endpoint = '/api/aow/settings/server-environment';
const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export function useServerEnvironment(active: boolean, onBusyChange: (busy: boolean) => void) {
  const [document, setDocument] = useState<ServerEnvironment>();
  const [values, setValues] = useState<ServerEnvironmentValues>({});
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (!active || document) return;
    let cancelled = false;
    setLoading(true);
    void aowRequest<ServerEnvironment>(endpoint, { cache: 'no-store' })
      .then(next => {
        if (cancelled) return;
        setDocument(next); setValues(readServerEnvironment(next.content, next.platform)); setError('');
      })
      .catch(reason => { if (!cancelled) setError(message(reason)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [active, document]);

  const original = document ? readServerEnvironment(document.content, document.platform) : {};
  const dirty = !!document && serverEnvironmentFields.some(({ key }) => (values[key] ?? '') !== (original[key] ?? ''));
  const fieldErrors = validateServerEnvironment(values);
  const reload = async () => {
    if (dirty && !window.confirm('重新读取会放弃 server.env 的未保存修改，是否继续？')) return;
    setLoading(true); onBusyChange(true); setError(''); setSaved(false);
    try {
      const next = await aowRequest<ServerEnvironment>(endpoint, { cache: 'no-store' });
      setDocument(next); setValues(readServerEnvironment(next.content, next.platform));
    } catch (reason) { setError(message(reason)); }
    finally { setLoading(false); onBusyChange(false); }
  };
  const save = async () => {
    if (!document || Object.keys(fieldErrors).length) return;
    onBusyChange(true); setError(''); setSaved(false);
    try {
      const content = writeServerEnvironment(document.content, values, document.platform);
      const next = await aowRequest<ServerEnvironment>(endpoint, {
        method: 'PUT', body: JSON.stringify({ content, revision: document.revision }),
      });
      setDocument(next); setValues(readServerEnvironment(next.content, next.platform)); setSaved(true);
    } catch (reason) { setError(message(reason)); }
    finally { onBusyChange(false); }
  };
  const edit = (key: string, value: string) => {
    setValues(current => ({ ...current, [key]: value }));
    setError(''); setSaved(false);
  };
  return { document, values, fieldErrors, loading, error, saved, dirty, reload, save, edit };
}

export function ServerEnvironmentFields({ environment, busy }: {
  environment: ReturnType<typeof useServerEnvironment>; busy: boolean;
}) {
  const { document, values, fieldErrors, loading, error, saved, reload, save, edit } = environment;
  return <>
    <h3>AoW 服务环境变量</h3>
    <div className="server-environment-actions">
      <div className="environment-settings-toolbar">
        {document ? <code className="server-environment-path" title={document.path}>{document.path}</code> : null}
        <div className="environment-settings-buttons">
          <button type="button" className="project-aow-dialog-button" disabled={loading || busy} onClick={() => void reload()}>重新读取文件</button>
          <button type="button" className="project-aow-dialog-button primary" disabled={loading || busy || !document || !!Object.keys(fieldErrors).length} onClick={() => void save()}>保存 server.env</button>
        </div>
      </div>
      {loading ? <p role="status">正在读取 server.env…</p> : null}
      {saved ? <p role="status">server.env 已保存。</p> : null}
      {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
    </div>
    <div className="server-environment-groups">
      {serverEnvironmentGroups.map(group => <table className="server-environment-table" key={group.title}>
        <caption>{group.title}</caption>
        <thead><tr><th scope="col">配置项（Key）</th><th scope="col">说明</th><th scope="col">Value 示例</th><th scope="col">当前值</th></tr></thead>
        <tbody>{group.fields.map(field => <tr key={field.key}>
          <th scope="row"><label htmlFor={`server-env-${field.key}`}><code>{field.key}</code></label></th>
          <td id={`server-env-${field.key}-description`}>{field.description}</td>
          <td><code>{field.example}</code></td>
          <td><div className="project-aow-dialog-field">
            <input id={`server-env-${field.key}`} aria-label={field.key} aria-describedby={`server-env-${field.key}-description${fieldErrors[field.key] ? ` server-env-${field.key}-error` : ''}`} aria-invalid={!!fieldErrors[field.key]}
              spellCheck={false} autoCapitalize="off" autoCorrect="off" autoComplete="off" inputMode={field.key === 'AOW_SERVER_PORT' || field.key.startsWith('AOW_AUTH_SESSION_') ? 'numeric' : 'text'}
              value={values[field.key] ?? ''} disabled={loading || busy || !document} onChange={event => edit(field.key, event.target.value)} placeholder="未配置"
              onKeyDown={event => { if (event.key === 'Enter' && !event.nativeEvent.isComposing) { event.preventDefault(); void save(); } }} />
            {fieldErrors[field.key] ? <small className="server-environment-error" id={`server-env-${field.key}-error`}>{fieldErrors[field.key]}</small> : null}
          </div></td>
        </tr>)}</tbody>
      </table>)}
    </div>
  </>;
}
