import { useEffect, useRef, useState } from 'react';
import { Check, ChevronDown, LogOut, Plus } from 'lucide-react';
import { logout } from './features/auth/authApi';
import type { Org, OrgState } from './orgs';

export function OrgSwitcher({ current, orgs, onSelect, onCreate }: {
  current: Org; orgs: OrgState['orgs']; onSelect: (id: string) => Promise<void>; onCreate: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const busyRef = useRef(false);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => { if (!container.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', outside);
    return () => document.removeEventListener('pointerdown', outside);
  }, [open]);
  const run = async (action: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError('');
    try { await action(); setOpen(false); trigger.current?.focus(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : '操作失败，请重试。'); }
    finally { busyRef.current = false; setBusy(false); }
  };
  return <div className="orbit-org-switcher" ref={container} onKeyDown={event => {
    if (event.key === 'Escape') { setOpen(false); trigger.current?.focus(); }
  }}>
    <button className="orbit-org-trigger" type="button" ref={trigger} aria-label={`切换组织，当前 ${current.name}`} aria-expanded={open} aria-controls="orbit-org-options" onClick={() => { setOpen(!open); setError(''); }}>
      <span className="orbit-org-avatar" aria-hidden="true">{[...current.name][0]}</span><strong>{current.name}</strong><ChevronDown size={14} aria-hidden="true" />
    </button>
    {open && <div id="orbit-org-options" className="orbit-org-popover">
      <div className="orbit-org-list" role="group" aria-label="组织列表">{orgs.map(org => <button type="button" key={org.id} aria-pressed={org.id === current.id} disabled={busy} onClick={() => { void run(() => onSelect(org.id)); }}>
        <span className="orbit-org-avatar" aria-hidden="true">{[...org.name][0]}</span><span>{org.name}</span>{org.id === current.id && <Check size={14} aria-hidden="true" />}
      </button>)}</div>
      <div className="orbit-org-actions"><button type="button" disabled={busy} onClick={() => { setOpen(false); onCreate(); }}><Plus size={15} aria-hidden="true" />创建组织</button>
        <button type="button" disabled={busy} onClick={() => { void run(async () => { await logout(); window.dispatchEvent(new Event('orbit:unauthorized')); }); }}><LogOut size={15} aria-hidden="true" />退出登录</button></div>
      {error && <p className="orbit-error" role="alert">{error}</p>}
    </div>}
  </div>;
}
