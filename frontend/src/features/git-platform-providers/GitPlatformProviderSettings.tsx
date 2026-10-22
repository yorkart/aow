import { t } from '../../lib/i18n';
import { lazy, Suspense, useEffect, useId, useRef, useState } from 'react';
import { Trash2, Upload } from 'lucide-react';
import { appUrl } from '../../lib/basePath';
import { AowSelect } from '../../components/AowSelect';
import { gitPlatformApi } from './api';
import type { GitPlatformProvider, GitPlatformProviderSettings as Settings } from './types';
import { MarkdownContent } from '../../components/MarkdownContent';
import protocol from './git-platform-providers.md?raw';
import './GitPlatformProviderSettings.css';

const ScriptPreview = lazy(() => import('./GitPlatformScriptPreview'));
const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export function GitPlatformProviderSettings({ active, onBusyChange, onDirtyChange }: {
  active: boolean; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void;
}) {
  const providerSelectId = useId();
  const [saved, setSaved] = useState<Settings>();
  const [loadRevision, setLoadRevision] = useState(0);
  const [draft, setDraft] = useState<Settings>();
  const [selected, setSelected] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const upload = useRef<HTMLInputElement>(null);
  const dirty = JSON.stringify(saved) !== JSON.stringify(draft);
  useEffect(() => { onDirtyChange(dirty); }, [dirty, onDirtyChange]);
  useEffect(() => { if (active) onBusyChange(busy); }, [active, busy, onBusyChange]);
  useEffect(() => {
    if (!active || draft) return;
    let live = true;
    setBusy(true);
    void gitPlatformApi.providers().then(value => { if (live) { setSaved(value); setDraft(value); } })
      .catch(reason => { if (live) setError(message(reason)); }).finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [active, draft, loadRevision]);
  const provider = draft?.providers[selected];
  const update = (patch: Partial<GitPlatformProvider>) => {
    setDraft(current => current ? { ...current, providers: current.providers.map((p, index) => index === selected ? { ...p, ...patch } : p) } : current);
    setStatus(''); setError('');
  };
  const save = async () => {
    if (!draft) return;
    setBusy(true); setError(''); setStatus('');
    try { const next = await gitPlatformApi.saveProviders(draft); setDraft(next); setSaved(next); setStatus(t("git_platform.provider_configuration_and_script_saved")); window.dispatchEvent(new Event('aow-git-platform-providers-changed')); }
    catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const validate = async () => {
    if (!provider) return;
    setBusy(true); setError(''); setStatus('');
    try { const metadata = await gitPlatformApi.testProvider(provider); update({ name: metadata.name, hosts: metadata.hosts }); setStatus(t("git_platform.read_capabilities_validated")); }
    catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const readFile = async (file?: File) => {
    if (!file || !draft) return;
    setBusy(true); setError(''); setStatus('');
    try {
      if (file.size > 1024 * 1024) throw new Error(t("git_platform.script_files_must_not_exceed_1_mib"));
      if (!/^[A-Za-z0-9][A-Za-z0-9._-]*\.mjs$/.test(file.name) || file.name.length > 100) throw new Error(t("git_platform.mjs_filename_required"));
      const index = draft.providers.findIndex(provider => provider.id.toLowerCase() === file.name.toLowerCase());
      if (index >= 0 && draft.providers[index].id !== file.name) throw new Error(t("git_platform.filename_case_conflict"));
      const script = await file.text();
      const metadata = await gitPlatformApi.testProvider({ id: file.name, script });
      const next: GitPlatformProvider = { id: file.name, script, name: metadata.name, hosts: metadata.hosts, enabled: index < 0 || draft.providers[index].enabled };
      setDraft({ ...draft, providers: index < 0 ? [...draft.providers, next] : draft.providers.map((provider, i) => i === index ? next : provider) });
      setSelected(index < 0 ? draft.providers.length : index);
      setStatus(t("git_platform.file_loaded_preview_below_changes_apply_after_saving"));
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); if (upload.current) upload.current.value = ''; }
  };
  return <section className="git-platform-provider-settings project-aow-dialog-form" aria-label={t("git_platform.settings_title")}>
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>{t("git_platform.title")}</h2><p>{t("git_platform.match_providers_by_git_remote_domain_scripts_implement_the")}</p></div></div>
      <div className="git-platform-provider-toolbar">
        <div className="git-platform-provider-picker">
          <label htmlFor={providerSelectId}>{t("navigation.provider")}</label>
          <AowSelect id={providerSelectId} label={t("git_platform.choose_provider")} disabled={busy || !draft?.providers.length} value={String(selected)}
            options={draft?.providers.map((provider, index) => ({ value: String(index), label: `${provider.name || provider.id}${provider.enabled ? '' : t("git_platform.disabled")}` })) ?? []}
            onChange={value => { setSelected(Number(value)); setStatus(''); }} />
        </div>
        <button className="project-aow-dialog-button" disabled={busy || !draft} onClick={() => upload.current?.click()}><Upload size={14} />{t("git_platform.upload_script")}</button>
        <input ref={upload} type="file" aria-label={t("git_platform.upload_provider_script")} accept=".mjs" hidden onChange={e => void readFile(e.target.files?.[0])} />
        {provider ? <button className="project-aow-dialog-button" disabled={busy} onClick={() => { setDraft({ ...draft!, providers: draft!.providers.filter((_, i) => i !== selected) }); setSelected(0); setStatus(''); }}><Trash2 size={14} />{t("notifications.remove")}</button> : null}
      </div>
      {provider ? <>
        <div className="git-platform-provider-fields">
          <label className="project-aow-dialog-field"><span>{t("navigation.provider_id")}</span><input aria-label={t("navigation.provider_id")} value={provider.id} readOnly /></label>
          <label className="project-aow-dialog-field"><span>{t("aow.display_name")}</span><input aria-label={t("git_platform.provider_display_name")} value={provider.name} readOnly /></label>
          <label className="project-aow-dialog-field git-platform-provider-hosts"><span>{t("git_platform.remote_domains_one_per_line")}</span><textarea aria-label={t("git_platform.remote_domains")} rows={3} spellCheck={false} value={provider.hosts.join('\n')} readOnly /></label>
        </div>
        <label className="project-aow-dialog-checkbox"><input type="checkbox" checked={provider.enabled} disabled={busy} onChange={e => update({ enabled: e.target.checked })} /><span>{t("git_platform.enable_this_provider")}</span></label>
        <div className="git-platform-provider-toolbar"><strong>{t("git_platform.mjs_script_read_only_preview")}</strong></div>
        <p className="project-aow-form-intro">{t("git_platform.scripts_are_preview_only_edit_locally_upload_a_replacement")}</p>
        {active ? <Suspense fallback={<p>{t("git_platform.loading_script_preview")}</p>}><ScriptPreview key={provider.id} value={provider.script} /></Suspense> : null}
      </> : draft ? <p>{t("git_platform.no_providers_configured_click_add_provider_to_begin")}</p> : busy ? <p>{t("git_platform.loading_providers")}</p> : <button onClick={() => { setError(''); setLoadRevision(value => value + 1); }}>{t("configuration.reload")}</button>}
      <details className="git-platform-provider-protocol"><summary>{t("git_platform.script_api_protocol_and_response_fields")}</summary><MarkdownContent readOnly text={protocol} /></details>
      <p className="project-aow-form-intro"><a href={appUrl('/api/public/git-platform-provider-schema')} target="_blank" rel="noreferrer">{t("git_platform.public_provider_schema")}</a></p>
      {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
      {status ? <p role="status">{status}</p> : null}
    </div>
    <footer className="project-aow-dialog-footer">
      {dirty ? <small>{t("configuration.unsaved_changes")}</small> : null}
      <button className="project-aow-dialog-button" disabled={busy || !dirty} onClick={() => { setDraft(saved); setSelected(0); setError(''); setStatus(''); }}>{t("acp.discard_changes")}</button>
      <button className="project-aow-dialog-button" disabled={busy || !provider} onClick={() => void validate()}>{t("git_platform.check_protocol")}</button>
      <button className="project-aow-dialog-button primary" disabled={busy || !draft || !dirty} onClick={() => void save()}>{busy ? t("acp.processing") : t("acp.save")}</button>
    </footer>
  </section>;
}
