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
  return <form className="inbox-label-editor" aria-label="配置标签" onSubmit={event => {
    event.preventDefault(); setBusy(true); setError('');
    void mutate(() => inboxApi.labels(revision, labels.map(label => ({ ...label, name: label.name.trim() })))).then(onClose).catch(reason => setError(inboxError(reason))).finally(() => setBusy(false));
  }}>
    <header><strong>配置标签</strong><button type="button" aria-label="关闭标签配置" disabled={busy} onClick={onClose}><X size={16} /></button></header>
    <div className="inbox-label-editor-list"><fieldset disabled={busy}>{labels.map((label, index) => <div key={label.id}>
      <input type="color" aria-label={`标签 ${index + 1} 颜色`} value={label.color} onChange={event => change(label.id, { color: event.target.value })} />
      <input aria-label={`标签 ${index + 1} 名称`} value={label.name} maxLength={40} required onChange={event => change(label.id, { name: event.target.value })} />
      <button type="button" aria-label={`删除标签 ${label.name}`} onClick={() => setLabels(items => items.filter(item => item.id !== label.id))}><Trash2 size={14} /></button>
    </div>)}</fieldset></div>
    {removedInUse && <p>保存后，已删除的标签也会从需求上移除。</p>}
    {error && <p role="alert" className="inbox-error">{error}<button type="button" onClick={() => { setLabels(data.labels.map(label => ({ ...label }))); setRevision(data.revision); setError(''); }}>重新载入标签</button></p>}
    <footer><button type="button" disabled={busy || labels.length >= 64} onClick={() => setLabels(items => [...items, { id: newInboxId(), name: '', color: '#94a3b8' }])}><Plus size={14} />添加标签</button><button className="inbox-primary" disabled={busy} type="submit">保存标签</button></footer>
  </form>;
}
