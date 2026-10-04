import { createRoot } from 'react-dom/client';
import { useEffect, useState } from 'react';
import { FloatingWorkspace } from '../src/aow/FloatingWorkspace';
import { FloatingWorkspaceProvider, useFloatingWorkspace } from '../src/aow/floatingWorkspaceState';
import '../src/styles.css';
import { InboxPanel } from '../src/features/inbox/InboxPanel';
import type { AowProject } from '../src/aow/types';
import type { AowAgent } from '../src/features/agents/types';
const projects = ['AoW', 'Website'].map((name, index) => ({ id: `project-${index}`, name, registered_path: `/repo/${index}`, common_git_dir: `/repo/${index}/.git`, notes_path: '/notes', worktrees: [
  { id: `worktree-feature-${index}`, project_id: `project-${index}`, path: `/repo/${index}/feature`, branch: 'feature/test', head: 'feature-head', is_main: false, detached: false, locked: false, prunable: false, color: 'default' as const },
  { id: `worktree-main-${index}`, project_id: `project-${index}`, path: `/repo/${index}`, branch: 'main', head: 'head', is_main: true, detached: false, locked: false, prunable: false, color: 'default' as const },
] })) satisfies AowProject[];
const agents = [{ id: 'codex', agent_type: 'codex', display_name: 'Codex', available: true }, { id: 'claude', agent_type: 'claude', display_name: 'Claude Code', available: true }, { id: 'traecli', agent_type: 'traecli', display_name: 'Trae CLI', available: false }, { id: 'custom-codex', agent_type: 'codex', display_name: 'Custom Codex', available: true }] as AowAgent[];
function FloatingPreview() {
  const floating = useFloatingWorkspace();
  useEffect(() => {
    floating.setGlobalRoot('/global');
    if (!floating.tabs.length) floating.openInbox();
  }, []);
  return <><button data-testid="refresh-projects" onClick={() => floating.retainWorkspaces(['/global', ...projects.flatMap(project => project.worktrees.map(worktree => worktree.path))])}>Refresh projects</button><FloatingWorkspace projects={projects} agents={agents} /></>;
}
function Preview() {
  const [terminal, setTerminal] = useState('');
  return <><style>{`html,body,#root { height:100%; margin:0; } body { font-family:-apple-system,BlinkMacSystemFont,sans-serif; background:#181b20; color:#d5d7dd; } * { box-sizing:border-box; } #root { max-width:920px; margin:auto; }`}</style>{new URLSearchParams(location.search).has('floating') ? <FloatingWorkspaceProvider><FloatingPreview /></FloatingWorkspaceProvider> : <InboxPanel visible projects={projects} agents={agents} onOpenTerminal={async id => { setTerminal(id); }} />}<output hidden={!terminal} data-opened-terminal={terminal}>{terminal}</output></>;
}
createRoot(document.getElementById('root')!).render(<Preview />);
