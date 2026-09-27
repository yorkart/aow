import { useEffect, useState } from 'react';
import { configurationApi } from './api';
import type { ConfigurationSettings as Settings, RepositoryVersions } from './types';
import './configuration-settings.css';

const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export function ConfigurationSettings({ active, onBusyChange, onDirtyChange }: {
  active: boolean; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void;
}) {
  const [saved, setSaved] = useState<Settings>();
  const [path, setPath] = useState('');
  const [versions, setVersions] = useState<RepositoryVersions>();
  const [selected, setSelected] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [loadRevision, setLoadRevision] = useState(0);
  const dirty = !!saved && (path.trim() !== saved.selection.config_repo || selected !== saved.selection.config_id);

  useEffect(() => { onDirtyChange(dirty); }, [dirty, onDirtyChange]);
  useEffect(() => { if (active) onBusyChange(busy); }, [active, busy, onBusyChange]);
  useEffect(() => {
    if (!active || saved) return;
    let live = true;
    setBusy(true);
    void (async () => {
      const settings = await configurationApi.settings();
      let choices: RepositoryVersions | undefined;
      try { choices = await configurationApi.versions(settings.selection.config_repo); }
      catch (reason) { if (live) setError(message(reason)); }
      if (live) {
        setSaved(settings); setPath(settings.selection.config_repo);
        setSelected(settings.selection.config_id); setVersions(choices);
      }
    })().catch(reason => { if (live) setError(message(reason)); })
      .finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [active, saved, loadRevision]);

  const inspect = async () => {
    setBusy(true); setError(''); setStatus('');
    try {
      const next = await configurationApi.versions(path.trim());
      setPath(next.config_repo); setVersions(next);
      setSelected(next.selected_id ?? (next.config_repo === saved?.selection.config_repo && next.config_ids.includes(saved.selection.config_id) ? saved.selection.config_id : ''));
    } catch (reason) { setVersions(undefined); setSelected(''); setError(message(reason)); }
    finally { setBusy(false); }
  };

  const save = async () => {
    if (!versions || !selected) return;
    setBusy(true); setError(''); setStatus('');
    try {
      const next = await configurationApi.save({ config_repo: versions.config_repo, config_id: selected });
      setSaved(next); setPath(next.selection.config_repo); setSelected(next.selection.config_id);
      setStatus(next.changed
        ? next.restart_required ? '配置已保存，重启服务后生效。' : '配置已保存，与当前运行版本一致。'
        : '配置未发生变化。');
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };

  return <section className="configuration-settings project-aow-dialog-form" aria-label="配置版本设置">
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>Configuration</h2><p>选择配置仓库和要使用的版本。</p></div></div>
      <form className="configuration-repository" onSubmit={event => { event.preventDefault(); void inspect(); }}>
        <label className="project-aow-dialog-field"><span>配置仓库目录</span><input aria-label="配置仓库目录" className="project-aow-dialog-monospace" value={path} disabled={busy} spellCheck={false} placeholder="/absolute/path/to/config-repo" onChange={event => {
          setPath(event.target.value); setVersions(undefined); setSelected(''); setError(''); setStatus('');
        }} /></label>
        <button type="submit" className="project-aow-dialog-button" disabled={busy || !path.trim()}>读取版本</button>
      </form>
      <p className="project-aow-form-intro">填写服务所在机器上的 Git 仓库目录，支持 ~/。也可以填写仓库下的 UUID 版本目录，读取后会自动选中该版本。</p>
      {versions ? versions.config_ids.length ? <fieldset className="configuration-versions" disabled={busy}>
        <legend>配置版本（单选）</legend>
        {versions.config_ids.map(id => <label className="configuration-version" key={id}>
          <input type="radio" name="config-version" value={id} checked={selected === id} onChange={() => { setSelected(id); setStatus(''); setError(''); }} />
          <span><code>{id}</code>{saved?.selection.config_repo === versions.config_repo && saved.selection.config_id === id ? <small>已保存</small> : null}</span>
        </label>)}
      </fieldset> : <p role="status">仓库中没有 UUID 版本目录，请选择包含配置版本的仓库。</p> : null}
      {saved ? <small className="project-aow-dialog-path-hint">当前运行版本：<code>{saved.active_selection.config_repo}/{saved.active_selection.config_id}</code></small> : null}
      {saved?.restart_required && !status ? <p role="status">已保存的配置与当前运行版本不同，重启服务后生效。</p> : null}
      {status ? <p role="status">{status}</p> : null}
      {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
      {!saved && !busy ? <button className="project-aow-dialog-button" onClick={() => { setError(''); setLoadRevision(value => value + 1); }}>重新加载</button> : null}
    </div>
    <footer className="project-aow-dialog-footer">
      {dirty ? <small>有未保存的修改</small> : null}
      <button className="project-aow-dialog-button primary" disabled={busy || !saved || !versions?.config_ids.includes(selected)} onClick={() => void save()}>{busy ? '处理中…' : '保存'}</button>
    </footer>
  </section>;
}
