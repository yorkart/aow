import { t } from '../../lib/i18n';
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
        ? next.restart_required ? t("configuration.configuration_saved_restart_the_service_to_apply") : t("configuration.configuration_saved_and_matches_the_running_version")
        : t("configuration.configuration_unchanged"));
    } catch (reason) { setError(message(reason)); }
    finally { setBusy(false); }
  };

  return <section className="configuration-settings project-aow-dialog-form" aria-label={t("configuration.configuration_version_settings")}>
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>{t("navigation.configuration")}</h2><p>{t("configuration.choose_a_configuration_repository_and_version")}</p></div></div>
      <form className="configuration-repository" onSubmit={event => { event.preventDefault(); void inspect(); }}>
        <label className="project-aow-dialog-field"><span>{t("configuration.configuration_repository_directory")}</span><input aria-label={t("configuration.configuration_repository_directory")} className="project-aow-dialog-monospace" value={path} disabled={busy} spellCheck={false} placeholder="/absolute/path/to/config-repo" onChange={event => {
          setPath(event.target.value); setVersions(undefined); setSelected(''); setError(''); setStatus('');
        }} /></label>
        <button type="submit" className="project-aow-dialog-button" disabled={busy || !path.trim()}>{t("configuration.read_versions")}</button>
      </form>
      <p className="project-aow-form-intro">{t("configuration.enter_a_git_repository_directory_on_the_server_is")}</p>
      {versions ? versions.config_ids.length ? <fieldset className="configuration-versions" disabled={busy}>
        <legend>{t("configuration.configuration_version_choose_one")}</legend>
        {versions.config_ids.map(id => <label className="configuration-version" key={id}>
          <input type="radio" name="config-version" value={id} checked={selected === id} onChange={() => { setSelected(id); setStatus(''); setError(''); }} />
          <span><code>{id}</code>{saved?.selection.config_repo === versions.config_repo && saved.selection.config_id === id ? <small>{t("configuration.saved")}</small> : null}</span>
        </label>)}
      </fieldset> : <p role="status">{t("configuration.no_configuration_version_directories_found_choose_a_repository_containing")}</p> : null}
      {saved ? <small className="project-aow-dialog-path-hint">{t("configuration.running_version")}<code>{saved.active_selection.config_repo}/{saved.active_selection.config_id}</code></small> : null}
      {saved?.restart_required && !status ? <p role="status">{t("configuration.the_saved_configuration_differs_from_the_running_version_restart")}</p> : null}
      {status ? <p role="status">{status}</p> : null}
      {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
      {!saved && !busy ? <button className="project-aow-dialog-button" onClick={() => { setError(''); setLoadRevision(value => value + 1); }}>{t("configuration.reload")}</button> : null}
    </div>
    <footer className="project-aow-dialog-footer">
      {dirty ? <small>{t("configuration.unsaved_changes")}</small> : null}
      <button className="project-aow-dialog-button primary" disabled={busy || !saved || !versions?.config_ids.includes(selected)} onClick={() => void save()}>{busy ? t("acp.processing") : t("acp.save")}</button>
    </footer>
  </section>;
}
