import type { Connector } from '../connectors/types';
import type { Skill, SkillEntry } from '../skills/types';
export interface Runtime { model: string | null; reasoning_effort: string | null; timeout_seconds: number }
export interface AgentVariable { id?: string; name: string; kind: 'variable' | 'secret'; value?: string; configured?: boolean }
export interface Agent {
  id: string; org_id: string; name: string; role: string; description: string; enabled: boolean;
  status: 'idle' | 'running' | 'disabled' | 'error'; status_message: string; running_count: number;
  connector: Connector; runtime: Runtime; revisions: { identity: number; runtime: number; skills: number; variables: number };
  instructions_version_id: string; instructions_updated_at: string; skills: Skill[]; variables: AgentVariable[];
  last_run_at: string | null; recent_tasks: { id: string; project_id: string; identifier: string; title: string; status: string; updated_at: string }[]; created_at: string; updated_at: string;
}
export interface RuntimeOptions { connector: Connector; models: { id: string; reasoning_efforts: string[] }[]; default_supported: boolean; timeout_min: number; timeout_max: number }
export interface Instructions { version_id: string; files: SkillEntry[] }
export type Tab = 'overview' | 'instructions' | 'skills' | 'runtime' | 'variables';
export const tabs: { id: Tab; label: string }[] = [{ id: 'overview', label: 'Overview' }, { id: 'instructions', label: 'Instructions' }, { id: 'skills', label: 'Skills' }, { id: 'runtime', label: 'Harness / Runtime' }, { id: 'variables', label: 'Secrets & variables' }];
export const statusNames = { idle: '空闲', running: '运行中', disabled: '已停用', error: '异常' };
export interface PanelProps { org: string; agent: Agent; onSaved: (agent: Agent) => void; onDirtyChange: (dirty: boolean) => void }
export type Navigate = (section: 'skills' | 'connectors' | 'projects', id?: string, taskId?: string) => void;
