import { useRef, useState, type FormEvent } from 'react';
import { Building2 } from 'lucide-react';

export function OrgForm({ initial = false, onCreate }: { initial?: boolean; onCreate: (name: string) => Promise<void> }) {
  const [name, setName] = useState('');
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const busy = useRef(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy.current || !name.trim()) return;
    busy.current = true;
    setSubmitting(true);
    setError('');
    try { await onCreate(name.trim()); }
    catch (reason) { setError(reason instanceof Error ? reason.message : '创建失败，请重试。'); }
    finally { busy.current = false; setSubmitting(false); }
  };
  return <>
    <div className="orbit-form-heading"><Building2 size={22} aria-hidden="true" /><h1 id="orbit-org-title">{initial ? '创建你的第一个组织' : '创建组织'}</h1></div>
    <p className="orbit-form-description">{initial ? '为你的团队或个人工作创建一个组织，开始使用 Orbit。' : '为新的团队或工作建立独立的组织。'}</p>
    <form className="orbit-org-form" onSubmit={event => { void submit(event); }}>
      <label htmlFor="orbit-org-name">组织名称</label>
      <input id="orbit-org-name" name="name" autoFocus autoComplete="organization" placeholder="例如：我的工作室" maxLength={80} value={name} required disabled={submitting} onChange={event => { setName(event.target.value); setError(''); }} />
      {error && <p className="orbit-error" role="alert">{error}</p>}
      <button className="orbit-primary" type="submit" disabled={submitting || !name.trim()}>{submitting ? '正在创建…' : initial ? '创建并进入工作空间' : '创建组织'}</button>
    </form>
  </>;
}
