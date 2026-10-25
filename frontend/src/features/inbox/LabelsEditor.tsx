import { t } from '../../lib/i18n';
import { useState } from 'react';
import { Plus, Trash2, X } from 'lucide-react';
import { inboxApi } from './api';
import { newInboxId } from './id';
import { inboxError, type InboxLabel, type InboxSnapshot } from './types';
export function LabelsEditor({ data, mutate, onClose }: { data: InboxSnapshot; mutate: <T>(action: () => Promise<T>) => Promise<T>; onClose: () => void }) {
  const [labels, setLabels] = useState(() => data.labels.map(label => ({ ...label })));
  const [revision, setRevision] = useState(data.revision);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const change = (id: string, patch: Partial<InboxLabel>) => setLabels(items => items.map(label => label.id === id ? { ...label, ...patch } : label));
  const removedInUse = data.items.some(item => item.label_ids.some(id => !labels.some(label => label.id === id)));
  return <form className="inbox-label-editor" aria-label={t("inbox.configure_labels")} onSubmit={event => {
    event.preventDefault(); setBusy(true); setError('');
    void mutate(() => inboxApi.labels(revision, labels.map(label => ({ ...label, name: label.name.trim() })))).then(onClose).catch(reason => setError(inboxError(reason))).finally(() => setBusy(false));
  }}>
    <header><strong>{t("inbox.configure_labels")}</strong><button type="button" aria-label={t("inbox.close_label_configuration")} disabled={busy} onClick={onClose}><X size={16} /></button></header>
    <div className="inbox-label-editor-list"><fieldset disabled={busy}>{labels.map((label, index) => <div key={label.id}>
      <input type="color" aria-label={t("inbox.label_color", { value0: index + 1 })} value={label.color} onChange={event => change(label.id, { color: event.target.value })} />
      <input aria-label={t("inbox.label_name", { value0: index + 1 })} value={label.name} maxLength={40} required onChange={event => change(label.id, { name: event.target.value })} />
      <button type="button" aria-label={t("inbox.delete_label", { value0: label.name })} onClick={() => setLabels(items => items.filter(item => item.id !== label.id))}><Trash2 size={14} /></button>
    </div>)}</fieldset></div>
    {removedInUse && <p>{t("inbox.saving_also_removes_deleted_labels_from_requirements")}</p>}
    {error && <p role="alert" className="inbox-error">{error}<button type="button" onClick={() => { setLabels(data.labels.map(label => ({ ...label }))); setRevision(data.revision); setError(''); }}>{t("inbox.reload_labels")}</button></p>}
    <footer><button type="button" disabled={busy || labels.length >= 64} onClick={() => setLabels(items => [...items, { id: newInboxId(), name: '', color: '#94a3b8' }])}><Plus size={14} />{t("inbox.add_label")}</button><button className="inbox-primary" disabled={busy} type="submit">{t("inbox.save_labels")}</button></footer>
  </form>;
}
