import { t } from '../../lib/i18n';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { LoaderCircle, MousePointerClick, Play, X } from 'lucide-react';
import { automationApi } from './api';
import { errorMessage } from './presentation';
import type { AutomationTask } from './types';
import { variableNames } from './variables';

export function ManualRunDialog({ task, onClose, onStarted }: {
  task: AutomationTask;
  onClose: () => void;
  onStarted: (runId: string | null) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [workspacePath, setWorkspacePath] = useState('');
  const dynamic = task.workspace_mode === 'dynamic';
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const submitting = useRef(false);
  const names = variableNames(task.prompt_bindings);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    element?.querySelector<HTMLInputElement | HTMLTextAreaElement>('input, textarea')?.focus();
    return () => element?.close();
  }, []);
  const ready = (!dynamic || workspacePath.startsWith('/')) && names.every(name => Object.hasOwn(values, name) && !!values[name].trim());
  return createPortal(<dialog ref={dialog} className="automation-editor automation-run-dialog" aria-labelledby="manual-run-title"
    onCancel={event => { event.preventDefault(); if (!submitting.current) onClose(); }} onKeyDown={event => event.stopPropagation()}>
    <form onSubmit={async event => {
      event.preventDefault();
      if (!ready || submitting.current) return;
      submitting.current = true; setBusy(true); setError('');
      try {
        const result = await automationApi.run(task.id, task.revision, values, dynamic ? workspacePath : undefined);
        onStarted(result.run_id);
      } catch (reason) { setError(errorMessage(reason)); }
      finally { submitting.current = false; setBusy(false); }
    }}>
      <header><div><MousePointerClick /><h2 id="manual-run-title">{t("automations.run_manual_task")}</h2></div><button type="button" aria-label={t("aow.close")} disabled={busy} onClick={onClose}><X /></button></header>
      <div className="automation-run-fields">
        <p>{task.name}</p>
        {dynamic && <label><span>{t("automations.workspace_directory")}</span>
          <input aria-label={t("automations.workspace_directory")} required disabled={busy} value={workspacePath} placeholder={t("automations.enter_the_absolute_path_of_a_workspace_in_this")}
            onChange={event => setWorkspacePath(event.target.value)} />
          <small className="automation-hint">{t("automations.choose_an_existing_directory_for_this_run_such_as")}</small>
        </label>}
        {names.length > 0 && <p className="automation-hint">{t("automations.enter_variable_values_for_this_run_all_variables_are")}</p>}
        {names.map((name, index) => <label key={name} htmlFor={`manual-variable-${index}`}><span>{name}</span>
          <textarea id={`manual-variable-${index}`} required rows={2} disabled={busy} value={Object.hasOwn(values, name) ? values[name] : ''}
            onChange={event => setValues(current => ({ ...current, [name]: event.target.value }))} />
        </label>)}
      </div>
      {error ? <div className="automation-error" role="alert">{error}</div> : null}
      <footer><span /><div><button type="button" disabled={busy} onClick={onClose}>{t("aow.cancel")}</button><button type="submit" className="automation-primary" disabled={busy || !ready}>{busy ? <LoaderCircle className="automation-spin" /> : <Play />}{busy ? t("aow.submitting") : t("automations.run")}</button></div></footer>
    </form>
  </dialog>, document.body);
}
