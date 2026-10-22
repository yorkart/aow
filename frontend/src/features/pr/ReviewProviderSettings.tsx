import { t } from '../../lib/i18n';
import { lazy, Suspense, useEffect, useId, useRef, useState } from 'react';
import { Plus, Trash2, Upload } from 'lucide-react';
import { AowSelect } from '../../components/AowSelect';
import { prApi } from './api';
import type { ReviewProvider, ReviewProviderSettings as Settings } from './types';
import { MarkdownContent } from '../../components/MarkdownContent';
import protocol from './review-providers.md?raw';
import './ReviewProviderSettings.css';

const ScriptPreview = lazy(() => import('./ReviewScriptPreview'));
const message = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);

export function ReviewProviderSettings({ active, onBusyChange, onDirtyChange }: {
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
    void prApi.reviewProviders().then(value => { if (live) { setSaved(value); setDraft(value); } })
      .catch(reason => { if (live) setError(message(reason)); }).finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [active, draft, loadRevision]);
  const provider = draft?.providers[selected];
  const update = (patch: Partial<ReviewProvider>) => {
    setDraft(current => current ? { ...current, providers: current.providers.map((p, index) => index === selected ? { ...p, ...patch } : p) } : current);
    setStatus(''); setError('');
  };
  const add = () => {
    if (!draft) return;
    let suffix = draft.providers.length + 1;
    while (draft.providers.some(provider => provider.id === 'provider-' + suffix)) suffix += 1;
    const next: ReviewProvider = { id: 'provider-' + suffix, name: t("pr.new_provider"), enabled: true, hosts: ['git.example.com'], script: '' };
    setSelected(draft.providers.length);
    setDraft({ ...draft, providers: [...draft.providers, next] }); setStatus('');
  };
  const save = async () => {
    if (!draft) return;
    setBusy(true); setError(''); setStatus('');
    try { const next = await prApi.saveReviewProviders(draft); setDraft(next); setSaved(next); setStatus(t("pr.provider_configuration_and_script_saved")); window.dispatchEvent(new Event('aow-review-providers-changed')); }
    catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const validate = async () => {
    if (!provider) return;
    setBusy(true); setError(''); setStatus('');
    try { await prApi.testReviewProvider(provider); setStatus(t("pr.the_describe_protocol_check_passed_declaring_list_detail_and")); }
    catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };
  const readFile = async (file?: File) => {
    if (!file) return;
    setBusy(true); setError('');
    try {
      if (file.size > 1024 * 1024) throw new Error(t("pr.script_files_must_not_exceed_1_mib"));
      update({ script: await file.text() }); setStatus(t("pr.file_loaded_preview_below_changes_apply_after_saving"));
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); if (upload.current) upload.current.value = ''; }
  };
  return <section className="review-provider-settings project-aow-dialog-form" aria-label={t("pr.pull_requests_settings")}>
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>{t("navigation.pull_requests")}</h2><p>{t("pr.match_providers_by_git_remote_domain_scripts_implement_the")}</p></div></div>
      <div className="review-provider-toolbar">
        <div className="review-provider-picker">
          <label htmlFor={providerSelectId}>{t("navigation.provider")}</label>
          <AowSelect id={providerSelectId} label={t("pr.choose_provider")} disabled={busy || !draft?.providers.length} value={String(selected)}
            options={draft?.providers.map((provider, index) => ({ value: String(index), label: `${provider.name || provider.id}${provider.enabled ? '' : t("pr.disabled")}` })) ?? []}
            onChange={value => { setSelected(Number(value)); setStatus(''); }} />
        </div>
        <button className="project-aow-dialog-button" disabled={busy || !draft} onClick={add}><Plus size={14} />{t("pr.add_provider")}</button>
        {provider ? <button className="project-aow-dialog-button" disabled={busy} onClick={() => { setDraft({ ...draft!, providers: draft!.providers.filter((_, i) => i !== selected) }); setSelected(0); setStatus(''); }}><Trash2 size={14} />{t("notifications.remove")}</button> : null}
      </div>
      {provider ? <>
        <div className="review-provider-fields">
          <label className="project-aow-dialog-field"><span>{t("navigation.provider_id")}</span><input aria-label={t("navigation.provider_id")} value={provider.id} disabled={busy} onChange={e => update({ id: e.target.value })} /></label>
          <label className="project-aow-dialog-field"><span>{t("aow.display_name")}</span><input aria-label={t("pr.provider_display_name")} value={provider.name} disabled={busy} onChange={e => update({ name: e.target.value })} /></label>
          <label className="project-aow-dialog-field review-provider-hosts"><span>{t("pr.remote_domains_one_per_line")}</span><textarea aria-label={t("pr.remote_domains")} rows={3} spellCheck={false} value={provider.hosts.join('\n')} disabled={busy} onChange={e => update({ hosts: e.target.value.split('\n') })} /></label>
        </div>
        <label className="project-aow-dialog-checkbox"><input type="checkbox" checked={provider.enabled} disabled={busy} onChange={e => update({ enabled: e.target.checked })} />{t("pr.enable_this_provider")}</label>
        <div className="review-provider-toolbar"><strong>{t("pr.python_script_read_only_preview")}</strong><button className="project-aow-dialog-button" disabled={busy} onClick={() => upload.current?.click()}><Upload size={14} />{t("pr.upload_script")}</button>
          <input ref={upload} type="file" aria-label={t("pr.upload_provider_script")} accept=".py,.txt,text/plain,text/x-python" hidden onChange={e => void readFile(e.target.files?.[0])} /></div>
        <p className="project-aow-form-intro">{t("pr.scripts_are_preview_only_edit_locally_upload_a_replacement")}</p>
        {active ? <Suspense fallback={<p>{t("pr.loading_script_preview")}</p>}><ScriptPreview key={provider.id} value={provider.script} /></Suspense> : null}
      </> : draft ? <p>{t("pr.no_providers_configured_click_add_provider_to_begin")}</p> : busy ? <p>{t("pr.loading_providers")}</p> : <button onClick={() => { setError(''); setLoadRevision(value => value + 1); }}>{t("configuration.reload")}</button>}
      <details className="review-provider-protocol"><summary>{t("pr.script_api_protocol_and_response_fields")}</summary><MarkdownContent readOnly text={protocol} /></details>
      {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
      {status ? <p role="status">{status}</p> : null}
    </div>
    <footer className="project-aow-dialog-footer">
      {dirty ? <small>{t("configuration.unsaved_changes")}</small> : null}
      <button className="project-aow-dialog-button" disabled={busy || !dirty} onClick={() => { setDraft(saved); setSelected(0); setError(''); setStatus(''); }}>{t("acp.discard_changes")}</button>
      <button className="project-aow-dialog-button" disabled={busy || !provider} onClick={() => void validate()}>{t("pr.check_protocol")}</button>
      <button className="project-aow-dialog-button primary" disabled={busy || !draft || !dirty} onClick={() => void save()}>{busy ? t("acp.processing") : t("acp.save")}</button>
    </footer>
  </section>;
}
