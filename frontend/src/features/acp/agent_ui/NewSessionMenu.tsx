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
    {loading ? <div className="project-aow-menu-empty" role="status">正在加载 ACP Agent…</div>
      : error ? <>
        <div className="project-aow-menu-empty" role="alert" title={error}>ACP Agent 加载失败</div>
        <button type="button" onClick={() => setRevision(value => value + 1)}><RefreshCw /><span>重新加载 ACP Agent</span></button>
      </> : agents.length ? agents.map(agent => <button type="button" data-workspace-open key={agent.id}
        aria-label={`新建 ACP 会话：${agent.name}`} onClick={() => onCreate(agent.id)}>
        <AgentIcon agentId={agent.id.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} /><span>{agent.name}</span>
      </button>) : <div className="project-aow-menu-empty">暂无可用 ACP Agent<br />请在 Settings → ACP 安装。</div>}
  </>;
}
