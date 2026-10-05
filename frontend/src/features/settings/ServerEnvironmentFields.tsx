import { useEffect, useState } from 'react';
import { aowRequest } from '../../lib/aowRequest';

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
  const [content, setContent] = useState('');
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
        setDocument(next); setContent(next.content); setError('');
      })
      .catch(reason => { if (!cancelled) setError(message(reason)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [active, document]);

  const dirty = !!document && content !== document.content;
  const reload = async () => {
    if (dirty && !window.confirm('重新读取会放弃 server.env 的未保存修改，是否继续？')) return;
    setLoading(true); onBusyChange(true); setError(''); setSaved(false);
    try {
      const next = await aowRequest<ServerEnvironment>(endpoint, { cache: 'no-store' });
      setDocument(next); setContent(next.content);
    } catch (reason) { setError(message(reason)); }
    finally { setLoading(false); onBusyChange(false); }
  };
  const save = async () => {
    if (!document) return;
    onBusyChange(true); setError(''); setSaved(false);
    try {
      const next = await aowRequest<ServerEnvironment>(endpoint, {
        method: 'PUT', body: JSON.stringify({ content, revision: document.revision }),
      });
      setDocument(next); setContent(next.content); setSaved(true);
    } catch (reason) { setError(message(reason)); }
    finally { onBusyChange(false); }
  };
  const edit = (text: string) => {
    // Textareas normalize line endings; preserve an existing CRLF document.
    const crlf = document?.content.includes('\r\n') && !document.content.replaceAll('\r\n', '').includes('\n');
    setContent(crlf ? text.replaceAll('\n', '\r\n') : text);
    setError(''); setSaved(false);
  };
  return { document, content, loading, error, saved, dirty, reload, save, edit };
}

export function ServerEnvironmentFields({ environment, busy }: {
  environment: ReturnType<typeof useServerEnvironment>; busy: boolean;
}) {
  const { document, content, loading, error, saved, reload, save, edit } = environment;
  return <>
    <h3>AoW 服务环境变量</h3>
    <p className="project-aow-form-intro">直接编辑服务所在机器的 <code>~/.config/aow/server.env</code>，保留注释、引号和空行。每行填写 KEY=value，不写 export。</p>
    {document ? <small className="project-aow-dialog-path-hint">文件：<code title={document.path}>{document.path}</code>{document.exists ? '' : '（尚不存在，保存时创建）'}</small> : null}
    <label className="project-aow-dialog-field"><span>server.env 文件内容</span><textarea aria-label="server.env 文件内容" spellCheck={false} autoCapitalize="off" autoCorrect="off" rows={10} value={content} disabled={loading || busy || !document} onChange={event => edit(event.target.value)} placeholder={'# AoW 服务配置\nAOW_SERVER_HOST=127.0.0.1\nAOW_SERVER_PORT=8282'} /></label>
    <p className="project-aow-form-intro">保存仅更新文件，当前运行的服务保持原配置。</p>
    {document?.platform === 'linux' ? <p className="project-aow-form-intro">保存后在服务器执行 <code>systemctl --user restart aow-server.service</code> 生效。</p> : document?.platform === 'macos' ? <p className="project-aow-form-intro">保存后重新执行 AoW 安装命令以加载配置。LaunchDaemon 模式需按安装器提示由本地管理员重新登记；仅重启进程不会重新读取此文件。</p> : null}
    <p className="project-aow-form-intro">直接运行二进制时不读取此文件，请通过启动参数或进程环境配置。</p>
    <div className="project-aow-settings-heading"><button type="button" className="project-aow-dialog-button" disabled={loading || busy} onClick={() => void reload()}>重新读取文件</button><button type="button" className="project-aow-dialog-button primary" disabled={loading || busy || !document} onClick={() => void save()}>保存 server.env</button></div>
    {loading ? <p role="status">正在读取 server.env…</p> : null}
    {saved ? <p role="status">server.env 已保存，请按上述步骤加载配置。</p> : null}
    {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
  </>;
}
