import { useEffect, useState } from 'react';
import { Bot, Plus } from 'lucide-react';
import { t } from '../../lib/i18n';
import { useConfirmation } from '../../components/ConfirmationDialog';
import { rolesApi, rolesChanged, type AgentRole } from './api';
import { RoleEditor } from './RoleEditor';
import './roles.css';

export function RoleSettings({ active, onBusyChange, onDirtyChange }: { active: boolean; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void }) {
  const [roles, setRoles] = useState<AgentRole[]>([]);
  const [editing, setEditing] = useState<AgentRole | 'new'>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);
  const { confirm, confirmationDialog } = useConfirmation();
  useEffect(() => { if (active) onBusyChange(busy); }, [active, busy, onBusyChange]);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const reload = () => { void rolesApi.list().then(items => { if (!cancelled) { setRoles(items); setError(''); } }).catch(reason => { if (!cancelled) setError(String(reason)); }); };
    reload(); window.addEventListener(rolesChanged, reload);
    return () => { cancelled = true; window.removeEventListener(rolesChanged, reload); };
  }, [active]);
  const remove = async (role: AgentRole) => {
    if (!await confirm({ title: t('roles.remove'), description: t('roles.remove_hint'), confirmLabel: t('roles.remove'), danger: true })) return;
    setBusy(true); setError('');
    try { await rolesApi.remove(role.id); } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
  };
  if (editing) return <RoleEditor role={editing === 'new' ? undefined : editing} onBusyChange={setBusy} onDirtyChange={onDirtyChange} onClose={() => setEditing(undefined)} onSaved={() => { setEditing(undefined); setSaved(true); }} />;
  return <div className="project-aow-dialog-form">
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>{t('roles.title')}</h2><p>{t('roles.description')}</p></div><button className="project-aow-dialog-button" type="button" disabled={busy} onClick={() => setEditing('new')}><Plus />{t('roles.add')}</button></div>
      {saved && <p role="status">{t('roles.saved')}</p>}
      <div className="acp-registry">{roles.map(role => <article key={role.id}><Bot /><div><strong>{role.name}</strong><small>{role.runtime.mode.toUpperCase()}{role.description ? ` · ${role.description}` : ''}</small></div><button className="project-aow-dialog-button" type="button" disabled={busy} onClick={() => setEditing(role)}>{t('acp.edit')}</button><button className="project-aow-dialog-button" type="button" disabled={busy} onClick={() => void remove(role)}>{t('roles.remove')}</button></article>)}</div>
      {!roles.length && <p className="project-aow-empty">{t('roles.empty')}</p>}
      {error && <p role="alert" className="project-aow-error">{error}</p>}
    </div>
    {confirmationDialog}
  </div>;
}
