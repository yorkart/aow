import { useEffect, useState } from 'react';
import { agentsApi } from '../features/agents/api';
import type { AowAgent } from '../features/agents/types';
import { SettingsDialog } from '../features/settings/SettingsDialog';
import './mobile-settings.css';

export function MobileSettings({ onClose, onReload, onNodesChange }: {
  onClose: () => void; onReload: () => void; onNodesChange: (addresses: string[]) => void;
}) {
  const [agents, setAgents] = useState<AowAgent[]>([]);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    void agentsApi.agents().then(next => { if (active) setAgents(next); })
      .catch(reason => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, []);
  const reload = async () => {
    try { setAgents(await agentsApi.agents(true)); setError(''); }
    catch (reason) { setError(String(reason)); throw reason; }
    finally { onReload(); }
  };
  return <SettingsDialog mobile agents={agents} agentsError={error} onClose={onClose} onReload={reload} onNodesChange={onNodesChange} />;
}
