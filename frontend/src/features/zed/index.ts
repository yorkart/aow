// Stable AoW integration surface. Consumers must not import internal files.
export { AgentPanel as AcpPanel } from './agent_ui/AgentPanel';
export { ZedSettings as AcpSettings } from './settings/ZedSettings';
export { SessionTab as AcpSessionTab } from './agent_ui/SessionTab';
export { useAcpTabs } from './agent_ui/workspace_tabs';
export type { AcpTab } from './agent_ui/workspace_tabs';
export type { SessionInfo as AcpSessionInfo } from './types';
