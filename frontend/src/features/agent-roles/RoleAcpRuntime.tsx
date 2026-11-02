import { useEffect, useState } from 'react';
import { t } from '../../lib/i18n';
import { acpApi, settingsChanged } from '../acp/api';
import type { AgentInfo } from '../acp/types';
import type { HomeLayout } from './api';

export function RoleAcpRuntime({ layouts, value, onChange, disabled, onReadyChange }: {
  layouts: HomeLayout[]; value: string; onChange: (id: string) => void; disabled: boolean;
  onReadyChange: (ready: boolean) => void;
}) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [reload, setReload] = useState(0);
  const configured = agents.filter(agent => agent.configured && layouts.some(layout => agent.id.toLowerCase().split(/[^a-z0-9]+/).includes(layout)));
  const selected = configured.find(agent => agent.id === value);
  const ready = !loading && !!selected?.supported && !!selected.installed;
  useEffect(() => { onReadyChange(ready); return () => onReadyChange(false); }, [ready, onReadyChange]);
  useEffect(() => {
    let cancelled = false;
    const load = () => {
      setLoading(true); setError('');
      void acpApi.agents().then(items => { if (!cancelled) setAgents(items); })
        .catch(reason => { if (!cancelled) { setAgents([]); setError(String(reason)); } })
        .finally(() => { if (!cancelled) setLoading(false); });
    };
    load(); window.addEventListener(settingsChanged, load);
    return () => { cancelled = true; window.removeEventListener(settingsChanged, load); };
  }, [reload]);
  return <div className="role-acp-runtime">
    <label className="project-aow-dialog-field"><span>{t('roles.acp_runtime')}</span><select aria-label={t('roles.acp_runtime')} value={value} disabled={disabled || loading} onChange={event => onChange(event.target.value)}>
      <option value="">{t('roles.select_runtime')}</option>
      {value && !selected && <option value={value} disabled>{value} · {t('roles.runtime_unavailable')}</option>}
      {configured.map(agent => <option key={agent.id} value={agent.id} disabled={!agent.supported || !agent.installed}>{agent.name}{agent.installed && agent.supported ? '' : ` · ${t('roles.runtime_unavailable')}`}</option>)}
    </select></label>
    <p className="project-aow-form-intro">{t('roles.shared_acp')}</p>
    {!loading && !configured.some(agent => agent.supported && agent.installed) && <p className="project-aow-empty">{t('roles.acp_empty')}</p>}
    {error && <p className="project-aow-error" role="alert">{error}</p>}
    <button className="project-aow-dialog-button" type="button" disabled={disabled || loading} onClick={() => setReload(value => value + 1)}>{t('roles.reload_runtimes')}</button>
  </div>;
}
