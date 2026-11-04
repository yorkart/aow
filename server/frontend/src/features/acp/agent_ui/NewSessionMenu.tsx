import { t } from '../../../lib/i18n';
import { useEffect, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { AgentIcon } from '../../agents/AgentIcon';
import { acpApi, settingsChanged } from '../api';
import { failure, type AgentInfo } from '../types';

export function NewSessionMenu({ onCreate }: { onCreate: (agentId: string) => void }) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const changed = () => setRevision(value => value + 1);
    window.addEventListener(settingsChanged, changed);
    return () => window.removeEventListener(settingsChanged, changed);
  }, []);
  useEffect(() => {
    let cancelled = false;
    setLoading(true); setError('');
    void acpApi.agents().then(values => {
      if (!cancelled) setAgents(values.filter(agent => agent.installed && agent.supported));
    }).catch(reason => { if (!cancelled) setError(failure(reason)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [revision]);
  return <>
    <div className="project-aow-menu-label">ACP</div>
    {loading ? <div className="project-aow-menu-empty" role="status">{t("acp.loading_acp_agents")}</div>
      : error ? <>
        <div className="project-aow-menu-empty" role="alert" title={error}>{t("acp.failed_to_load_acp_agents")}</div>
        <button type="button" onClick={() => setRevision(value => value + 1)}><RefreshCw /><span>{t("acp.reload_acp_agents")}</span></button>
      </> : agents.length ? agents.map(agent => <button type="button" data-workspace-open key={agent.id}
        aria-label={t("acp.new_acp_session", { value0: agent.name })} onClick={() => onCreate(agent.id)}>
        <AgentIcon agentId={agent.id.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} /><span>{agent.name}</span>
      </button>) : <div className="project-aow-menu-empty">{t("acp.no_acp_agents_available")}<br />{t("acp.install_one_in_settings_acp")}</div>}
  </>;
}
