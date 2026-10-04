import type { AowProject } from '../../aow/types';
import type { MobileNavigate } from '../../mobile/mobileState';
import { useMobileResource } from '../../mobile/mobileState';
import { agentsApi } from '../agents/api';
import { terminalApi } from '../terminals/terminalApi';
import { InboxPanel } from './InboxPanel';
export function MobileInbox({ projects, navigate }: { projects: AowProject[]; navigate: MobileNavigate }) {
  const agents = useMobileResource('inbox-agents', () => agentsApi.agents());
  return <div className="mobile-inbox">
    {agents.error && <p role="alert" className="inbox-error">{agents.error}<button onClick={agents.reload}>重新加载 Agent</button></p>}
    <InboxPanel visible projects={projects} agents={agents.data ?? []} onOpenTerminal={async id => {
      const tab = await terminalApi.get(id);
      navigate({ workspace: tab.workspace_root, view: 'terminal', terminal: tab.id });
    }} />
  </div>;
}
