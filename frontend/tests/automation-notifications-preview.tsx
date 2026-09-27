import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { AutomationEditor } from '../src/features/automations/AutomationEditor';
import type { AutomationTask } from '../src/features/automations/types';
import type { AowProject } from '../src/aow/types';
import type { AowAgent } from '../src/features/agents/types';
import '../src/styles.css';
import '../src/features/automations/automations.css';

const project = { id: 'project', name: 'Project', registered_path: '/repo', common_git_dir: '/repo/.git', notes_path: '/notes',
  worktrees: [{ id: 'main', project_id: 'project', path: '/repo', branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false, color: 'default' }],
} satisfies AowProject;
const agents = [{ id: 'codex', agent_type: 'codex', display_name: 'Codex', source: 'configured', available: true,
  command: 'codex', executable: '/bin/codex', args: [], env: {} }] satisfies AowAgent[];
const task: AutomationTask = {
  id: '12345678', revision: 1, kind: 'scheduled', prompt_bindings: [], name: '每日检查', prompt: '检查任务', agent: 'codex', project_id: 'project', project_name: 'Project',
  workspace_mode: 'existing', workspace_path: '/repo', cleanup_worktree: false, base_branch: 'main', cron: '0 9 * * *',
  interval_seconds: null, max_concurrent_runs: 1, enabled: true, yolo: true, precheck_command: '', precheck_timeout_seconds: 60,
  failure_notification: new URLSearchParams(location.search).has('feishu') ? 'feishu' : null,
  created_at: '', updated_at: '', scheduler_error: null, next_run_at: null, last_run: null, is_running: false,
};
function Preview() {
  const [saved, setSaved] = useState<AutomationTask>();
  return saved ? <pre data-testid="saved">{JSON.stringify(saved)}</pre> : <AutomationEditor task={task} project={project} agents={agents} onClose={() => {}} onSaved={setSaved} />;
}
createRoot(document.getElementById('root')!).render(<StrictMode><Preview /></StrictMode>);
