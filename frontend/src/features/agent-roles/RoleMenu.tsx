import { useEffect, useState } from 'react';
import { Bot, RefreshCw } from 'lucide-react';
import { t } from '../../lib/i18n';
import { rolesApi, rolesChanged, type AgentRole } from './api';

export function RoleMenu({ onCreate, acpOnly = false }: { acpOnly?: boolean; onCreate: (role: AgentRole) => void }) {
  const [roles, setRoles] = useState<AgentRole[]>([]);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const changed = () => setRevision(value => value + 1);
    window.addEventListener(rolesChanged, changed);
    return () => window.removeEventListener(rolesChanged, changed);
  }, []);
  useEffect(() => {
    let cancelled = false;
    setError('');
    void rolesApi.list().then(items => { if (!cancelled) setRoles(items); }).catch(reason => { if (!cancelled) setError(String(reason)); });
    return () => { cancelled = true; };
  }, [revision]);
  const available = roles.filter(role => !acpOnly || role.runtime.mode === 'acp');
  return <>
    <div className="project-aow-menu-label">{t('roles.title')}</div>
    {available.map(role => <button key={role.id} data-workspace-open onClick={() => onCreate(role)}><Bot /><span>{role.name}</span><small>{role.runtime.mode.toUpperCase()}</small></button>)}
    {error ? <button title={error} onClick={() => setRevision(value => value + 1)}><RefreshCw /><span>{t('roles.reload')}</span></button> : !available.length && <div className="project-aow-menu-empty">{t('roles.empty')}</div>}
  </>;
}
