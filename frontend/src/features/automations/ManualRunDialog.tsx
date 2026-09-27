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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const submitting = useRef(false);
  const names = variableNames(task.prompt_bindings);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    element?.querySelector('textarea')?.focus();
    return () => element?.close();
  }, []);
  const ready = names.every(name => Object.hasOwn(values, name) && !!values[name].trim());
  return createPortal(<dialog ref={dialog} className="automation-editor automation-run-dialog" aria-labelledby="manual-run-title"
    onCancel={event => { event.preventDefault(); if (!submitting.current) onClose(); }} onKeyDown={event => event.stopPropagation()}>
    <form onSubmit={async event => {
      event.preventDefault();
      if (!ready || submitting.current) return;
      submitting.current = true; setBusy(true); setError('');
      try {
        const result = await automationApi.run(task.id, task.revision, values);
        onStarted(result.run_id);
      } catch (reason) { setError(errorMessage(reason)); }
      finally { submitting.current = false; setBusy(false); }
    }}>
      <header><div><MousePointerClick /><h2 id="manual-run-title">运行手动任务</h2></div><button type="button" aria-label="关闭" disabled={busy} onClick={onClose}><X /></button></header>
      <div className="automation-run-fields">
        <p>{task.name}</p><p className="automation-hint">填写本次执行的变量值，所有变量均为必填。</p>
        {names.map((name, index) => <label key={name} htmlFor={`manual-variable-${index}`}><span>{name}</span>
          <textarea id={`manual-variable-${index}`} required rows={2} disabled={busy} value={Object.hasOwn(values, name) ? values[name] : ''}
            onChange={event => setValues(current => ({ ...current, [name]: event.target.value }))} />
        </label>)}
      </div>
      {error ? <div className="automation-error" role="alert">{error}</div> : null}
      <footer><span /><div><button type="button" disabled={busy} onClick={onClose}>取消</button><button type="submit" className="automation-primary" disabled={busy || !ready}>{busy ? <LoaderCircle className="automation-spin" /> : <Play />}{busy ? '提交中…' : '运行'}</button></div></footer>
    </form>
  </dialog>, document.body);
}
