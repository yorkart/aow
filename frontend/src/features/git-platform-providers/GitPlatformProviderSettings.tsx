import { t } from '../../lib/i18n';
import { useEffect, useRef, useState } from 'react';
import { Trash2, Upload } from 'lucide-react';
import { gitPlatformApi } from './api';
import type { GitPlatformProvider, GitPlatformProviderSettings as Settings } from './types';
import './GitPlatformProviderSettings.css';

const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export function GitPlatformProviderSettings({ active, onBusyChange }: {
  active: boolean; onBusyChange: (busy: boolean) => void;
}) {
  const [settings, setSettings] = useState<Settings>();
  const [loadRevision, setLoadRevision] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const upload = useRef<HTMLInputElement>(null);
  useEffect(() => { if (active) onBusyChange(busy); }, [active, busy, onBusyChange]);
  useEffect(() => {
    if (!active || settings) return;
    let live = true;
    setBusy(true);
    void gitPlatformApi.providers().then(value => { if (live) setSettings(value); })
      .catch(reason => { if (live) setError(message(reason)); }).finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [active, settings, loadRevision]);
  const save = async (providers: GitPlatformProvider[]) => {
    const next = await gitPlatformApi.saveProviders({ revision: settings!.revision, providers });
    setSettings(next);
    window.dispatchEvent(new Event('aow-git-platform-providers-changed'));
  };
  const remove = async (id: string) => {
    if (!settings || busy) return;
    setBusy(true); setError(''); setStatus('');
    try {
      await save(settings.providers.filter(provider => provider.id !== id));
      setStatus(t("git_platform.provider_removed"));
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const readFile = async (file?: File) => {
    if (!file || !settings || busy) return;
    setBusy(true); setError(''); setStatus('');
    try {
      if (file.size > 1024 * 1024) throw new Error(t("git_platform.script_files_must_not_exceed_1_mib"));
      if (!/^[A-Za-z0-9][A-Za-z0-9._-]*\.mjs$/.test(file.name) || file.name.length > 100) throw new Error(t("git_platform.mjs_filename_required"));
      const index = settings.providers.findIndex(provider => provider.id.toLowerCase() === file.name.toLowerCase());
      if (index >= 0 && settings.providers[index].id !== file.name) throw new Error(t("git_platform.filename_case_conflict"));
      const next: GitPlatformProvider = { id: file.name, script: await file.text(), name: '', hosts: [] };
      await save(index < 0 ? [...settings.providers, next] : settings.providers.map((provider, i) => i === index ? next : provider));
      setStatus(t("git_platform.provider_uploaded"));
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); if (upload.current) upload.current.value = ''; }
  };
  return <section className="git-platform-provider-settings project-aow-dialog-form" aria-label={t("git_platform.settings_title")} aria-busy={busy}>
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading">
        <div><h2>{t("git_platform.title")}</h2></div>
        <button type="button" className="project-aow-dialog-button" disabled={busy || !settings} onClick={() => upload.current?.click()}><Upload size={14} />{t("git_platform.upload_script")}</button>
        <input ref={upload} type="file" aria-label={t("git_platform.upload_provider_script")} accept=".mjs" hidden onChange={e => void readFile(e.target.files?.[0])} />
      </div>
      {settings?.providers.length ? <table className="git-platform-provider-list" aria-label={t("git_platform.title")}>
        <thead><tr><th scope="col">{t("navigation.provider_id")}</th><th scope="col">{t("aow.display_name")}</th><th scope="col">{t("git_platform.remote_domains")}</th><th scope="col" aria-label={t("notifications.remove")} /></tr></thead>
        <tbody>{settings.providers.map(provider => <tr key={provider.id}>
          <td data-label={t("navigation.provider_id")}>{provider.id}</td>
          <td data-label={t("aow.display_name")}>{provider.name}</td>
          <td data-label={t("git_platform.remote_domains")}>{provider.hosts.map(host => <div key={host}>{host}</div>)}</td>
          <td className="git-platform-provider-actions"><button type="button" className="project-aow-dialog-button" disabled={busy} onClick={() => void remove(provider.id)}><Trash2 size={14} />{t("notifications.remove")}</button></td>
        </tr>)}</tbody>
      </table> : settings ? <p>{t("git_platform.no_providers_configured_click_add_provider_to_begin")}</p> : busy ? <p>{t("git_platform.loading_providers")}</p> : null}
      {error ? <div className="project-aow-error" role="alert">{error}<button type="button" className="project-aow-dialog-button" disabled={busy} onClick={() => { setError(''); setStatus(''); setSettings(undefined); setLoadRevision(value => value + 1); }}>{t("configuration.reload")}</button></div> : null}
      {status ? <p role="status">{status}</p> : null}
    </div>
  </section>;
}
