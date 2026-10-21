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
    <p className="project-aow-form-intro">填写下表的值，保存到服务所在机器的 <code>~/.config/aow/server.env</code>。未配置的项留空，示例不会自动保存；清空已有值会取消该项配置。路径填写绝对路径，无需添加引号或 export。</p>
    {document ? <small className="project-aow-dialog-path-hint">文件：<code title={document.path}>{document.path}</code>{document.exists ? '' : '（尚不存在，保存时创建）'}</small> : null}
    <div className="server-environment-actions">
      <div className="project-aow-settings-heading"><button type="button" className="project-aow-dialog-button" disabled={loading || busy} onClick={() => void reload()}>重新读取文件</button><button type="button" className="project-aow-dialog-button primary" disabled={loading || busy || !document || !!Object.keys(fieldErrors).length} onClick={() => void save()}>保存 server.env</button></div>
      {loading ? <p role="status">正在读取 server.env…</p> : null}
      {saved ? <p role="status">server.env 已保存，请按页面提示加载配置。</p> : null}
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
    <p className="project-aow-form-intro">保留文件中的注释和其他配置。代理变量的大小写项独立保存，不自动同步。</p>
    <p className="project-aow-form-intro">保存仅更新文件，当前运行的服务保持原配置。</p>
    {document?.platform === 'linux' ? <p className="project-aow-form-intro">保存后在服务器执行 <code>systemctl --user restart aow-server.service</code> 生效。</p> : document?.platform === 'macos' ? <p className="project-aow-form-intro">HTTP_PROXY、HTTPS_PROXY、ALL_PROXY、NO_PROXY（含小写形式）在 Web 服务启动时读取；仅修改代理可直接重新执行 AoW 安装命令，由服务账户重启 Web 服务生效。其他配置仍由安装器加载，LaunchDaemon 模式可能需要本地管理员重新登记。</p> : null}
    <p className="project-aow-form-intro">直接运行二进制时不读取此文件，请通过启动参数或进程环境配置。</p>
  </>;
}
