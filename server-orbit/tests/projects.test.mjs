import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { access, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, delimiter, dirname, join } from 'node:path';
import { test } from 'node:test';
import { setAccount } from '../dist/auth.js';
import { configuration } from '../dist/config.js';
import { openDatabase } from '../dist/database.js';
import { createOrbitServer } from '../dist/http.js';
import { Connectors } from '../dist/connectors/service.js';
import { Agents } from '../dist/agents/service.js';
import { AgentRuns } from '../dist/agents/runtime.js';
import { Skills } from '../dist/skills/service.js';
import { SkillProjects } from '../dist/skills/projects.js';
import { git, parseWorktrees } from '../dist/projects/git.js';
import { Projects } from '../dist/projects/service.js';
import { Workspaces } from '../dist/projects/workspaces.js';
import { Tasks } from '../dist/projects/tasks.js';
import { TaskWorkspaces } from '../dist/projects/task-workspaces.js';

const code = expected => error => error.code === expected;
async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'orbit-projects-')); const state = join(directory, 'state'); const repo = join(directory, 'repo with 空格');
  await mkdir(repo); const database = openDatabase(state); const org = randomUUID(); const other = randomUUID();
  for (const id of [org, other]) database.prepare('INSERT INTO orgs VALUES (?, ?, ?)').run(id, id, new Date().toISOString());
  const projects = new Projects(database); const workspaces = new Workspaces(projects); const tasks = new Tasks(projects, workspaces);
  const runGit = async (path, args) => { const result = await git(path, args); assert.equal(result.ok, true, `git ${args.join(' ')} failed`); return result.output.trimEnd(); };
  const init = async () => { await runGit(repo, ['init', '-b', 'main']); await runGit(repo, ['config', 'user.email', 'test@example.invalid']); await runGit(repo, ['config', 'user.name', 'Orbit tests']); await writeFile(join(repo, 'README.md'), '# Repository'); await runGit(repo, ['add', 'README.md']); await runGit(repo, ['commit', '-m', 'Initial']); };
  let closed = false;
  t.after(async () => { if (!closed) database.close(); await rm(directory, { recursive: true, force: true }); });
  return { directory, state, repo, database, org, other, projects, workspaces, tasks, runGit, init, close() { database.close(); closed = true; } };
}
async function agentFixture(t, app) {
  const home = join(app.directory, 'operator'); const bin = join(app.directory, 'bin'); await mkdir(join(home, '.codex'), { recursive: true }); await mkdir(bin);
  const script = await readFile(new URL('./fixtures/connector-cli.mjs', import.meta.url), 'utf8'); await writeFile(join(bin, 'codex'), `#!${process.execPath}\n${script}`, { mode: 0o700 });
  await writeFile(join(home, '.codex/auth.json'), JSON.stringify({ OPENAI_API_KEY: 'project-test-local-identity' }));
  const options = { localHome: home, env: { ...process.env, PATH: bin + delimiter + process.env.PATH, CODEX_HOME: join(home, '.codex') } };
  const connectors = new Connectors(app.database, app.state, options); t.after(() => connectors.close());
  const connector = await connectors.save(app.org, { name: 'Local', provider: 'openai', executor: 'codex', auth_method: 'local', account_note: '', connect: true });
  const agents = new Agents(app.database, app.state, connectors, new Skills(app.database, app.state)); const agent = await agents.create(app.org, { name: 'Reviewer', connector_id: connector.id });
  return { agent, agents, options, runs: new AgentRuns(agents) };
}

test('project configuration supports multiple empty Codebases, canonical paths, validation, org isolation and optimistic saves', async t => {
  const app = await fixture(t); let first = await app.projects.save(app.org, { name: 'Plan', description: 'No code needed' }); const second = await app.projects.save(app.org, { name: 'Second' });
  assert.equal(first.codebase_path, null); assert.equal(second.directory, null); assert.ok(first.created_at); assert.equal(first.task_count, 0);
  await assert.rejects(app.projects.save(app.org, { name: 'Relative', codebase_path: 'relative' }), code('invalid_codebase'));
  await assert.rejects(app.projects.save(app.org, { name: 'Missing', codebase_path: join(app.directory, 'missing') }), code('invalid_codebase'));
  const file = join(app.directory, 'file'); await writeFile(file, 'File'); await assert.rejects(app.projects.save(app.org, { name: 'File', codebase_path: file }), code('invalid_codebase'));
  const alias = join(app.directory, 'alias'); await symlink(app.repo, alias, 'dir');
  first = await app.projects.save(app.org, { name: 'Updated', description: 'Code', codebase_path: alias, revision: first.revision }, first.id); assert.equal(first.codebase_path, app.repo); assert.equal(first.git.is_git, false);
  await assert.rejects(app.projects.save(app.org, { name: 'Stale', revision: 1 }, first.id), code('project_conflict'));
  await assert.rejects(app.projects.save(app.org, { name: 'Duplicate', codebase_path: app.repo }), code('codebase_in_use'));
  const legacy = await app.projects.register(app.org, { name: 'Legacy', directory: alias }); assert.equal(legacy.id, first.id);
  assert.throws(() => app.projects.get(app.other, first.id), code('project_not_found')); assert.deepEqual(app.projects.list(app.other), []);
  first = await app.projects.save(app.org, { name: 'Updated', codebase_path: null, directory: app.repo, revision: first.revision }, first.id); assert.equal(first.directory, null); await access(app.repo);
  const listing = await app.projects.directories(app.directory); assert.ok(listing.directories.some(item => item.path === app.repo)); assert.equal(listing.directories.some(item => item.name === 'file' || item.name === 'alias'), false);
});

test('existing project database layout upgrades without changing IDs or losing Skills and Agent run references', async t => {
  const app = await fixture(t); const project = await app.projects.save(app.org, { name: 'Legacy', codebase_path: app.repo }); const skills = new Skills(app.database, app.state);
  const skill = await skills.create(app.org, { name: 'legacy-skill', description: 'Legacy', instructions: 'Instructions' }); app.database.prepare('UPDATE skills SET source_project_id = ? WHERE id = ?').run(project.id, skill.id);
  const { agent, runs } = await agentFixture(t, app); await runs.prepare(app.org, agent.id, 'legacy-run', project.id);
  app.database.exec(`PRAGMA foreign_keys = OFF; BEGIN; CREATE TABLE projects_old (id TEXT PRIMARY KEY, org_id TEXT NOT NULL REFERENCES orgs(id), name TEXT NOT NULL, directory TEXT NOT NULL, UNIQUE(org_id,directory), UNIQUE(org_id,id)); INSERT INTO projects_old SELECT id,org_id,name,directory FROM projects; DROP TABLE projects; ALTER TABLE projects_old RENAME TO projects; COMMIT;`);
  app.close(); const database = openDatabase(app.state); t.after(() => database.close());
  assert.equal(new Projects(database).get(app.org, project.id).name, 'Legacy'); assert.equal(database.prepare('SELECT source_project_id FROM skills WHERE id = ?').get(skill.id).source_project_id, project.id);
  assert.equal(database.prepare('SELECT project_id FROM agent_runs WHERE run_id = ?').get('legacy-run').project_id, project.id); assert.deepEqual(database.prepare('PRAGMA foreign_key_check').all(), []);
  assert.equal((await new Projects(database).save(app.org, { name: 'Now optional', codebase_path: '', revision: 1 }, project.id)).directory, null);
});

test('workspace discovery reads real Git worktrees with spaces, deduplicates paths, tracks branch changes and leaves usage and repository untouched', async t => {
  const app = await fixture(t); await app.init(); const linked = join(app.directory, 'linked 工作区'); const detached = join(app.directory, 'detached');
  await app.runGit(app.repo, ['worktree', 'add', '-b', 'feature/review', linked]); await app.runGit(app.repo, ['worktree', 'add', '--detach', detached, 'HEAD']);
  const before = await app.runGit(app.repo, ['worktree', 'list', '--porcelain', '-z']); const project = await app.projects.save(app.org, { name: 'Repository', codebase_path: app.repo });
  let result = await app.workspaces.list(app.org, project.id); assert.equal(result.workspaces.length, 3); assert.equal(result.workspaces[0].directory, app.repo); assert.equal(result.workspaces[0].is_primary, true);
  assert.equal(new Set(result.workspaces.map(row => row.directory)).size, 3); assert.ok(result.workspaces.every(row => row.available && row.last_used_at === null && row.tasks.length === 0));
  const workspace = result.workspaces.find(row => row.directory === linked); assert.equal(workspace.branch, 'feature/review');
  assert.equal(result.workspaces.find(row => row.directory === detached).branch, null); assert.match(result.workspaces.find(row => row.directory === detached).head, /^[0-9a-f]{40,64}$/);
  assert.equal(await app.runGit(app.repo, ['worktree', 'list', '--porcelain', '-z']), before);
  const task = await app.tasks.save(app.org, { project_id: project.id, title: 'Review', workspace_id: workspace.id });
  await app.runGit(linked, ['branch', '-m', 'feature/renamed']); result = await app.workspaces.list(app.org, project.id);
  const renamed = result.workspaces.find(row => row.directory === linked); assert.equal(renamed.id, workspace.id); assert.equal(renamed.branch, 'feature/renamed'); assert.equal(renamed.tasks[0].id, task.id);
  app.workspaces.used(app.org, project.id, workspace.id); result = await app.workspaces.list(app.org, project.id); assert.equal(result.workspaces[1].id, workspace.id);
  await rm(linked, { recursive: true, force: true }); result = await app.workspaces.list(app.org, project.id); assert.equal(result.workspaces.find(row => row.id === workspace.id).available, false);
  await assert.rejects(app.workspaces.resolve(app.org, project.id, workspace.id), code('workspace_unavailable'));
  assert.equal(parseWorktrees('worktree /tmp/a\0HEAD abc\0branch refs/heads/main\0locked reason\0\0')[0].locked, true);
});

test('tasks are real org/project records with unique identifiers, filters, Agents, workspace IDs and stale-edit protection', async t => {
  const app = await fixture(t); const { agent, agents } = await agentFixture(t, app); const project = await app.projects.save(app.org, { name: 'Tasks' }); const second = await app.projects.save(app.org, { name: 'Other project', codebase_path: app.repo });
  const [first, other] = await Promise.all([app.tasks.save(app.org, { project_id: project.id, title: 'Review implementation', agent_id: agent.id }), app.tasks.save(app.org, { project_id: second.id, title: 'Documentation' })]);
  assert.notEqual(first.identifier, other.identifier); assert.equal(first.agent.name, 'Reviewer'); assert.equal(first.workspace, null);
  const edited = await app.tasks.save(app.org, { revision: first.revision, title: 'Review updated', status: 'done' }, first.id); assert.equal(edited.agent_id, agent.id);
  assert.equal(app.tasks.list(app.org, project.id, 'updated', 'done').length, 1); assert.equal(app.tasks.list(app.org, second.id).length, 1); assert.equal(app.projects.get(app.org, project.id).task_count, 1);
  assert.equal(agents.get(app.org, agent.id).recent_tasks[0].id, first.id);
  await assert.rejects(app.tasks.save(app.org, { revision: first.revision, title: 'Stale' }, first.id), code('task_conflict'));
  assert.throws(() => app.tasks.get(app.other, first.id), code('task_not_found'));
  const foreign = await app.projects.save(app.other, { name: 'Foreign', codebase_path: app.repo }); const workspace = (await app.workspaces.list(app.other, foreign.id)).workspaces[0];
  await assert.rejects(app.tasks.save(app.org, { project_id: project.id, title: 'Wrong workspace', workspace_id: workspace.id }), code('workspace_not_found'));
  await assert.rejects(app.tasks.save(app.other, { project_id: foreign.id, title: 'Wrong Agent', agent_id: agent.id }), code('invalid_task_agent'));
});

test('changing or clearing Codebase preserves task-bound workspace IDs, local modifications, and imports require an actual Codebase', async t => {
  const app = await fixture(t); let project = await app.projects.save(app.org, { name: 'Original', codebase_path: app.repo });
  const workspace = (await app.workspaces.list(app.org, project.id)).workspaces[0]; const task = await app.tasks.save(app.org, { project_id: project.id, title: 'Bound task', workspace_id: workspace.id }); await writeFile(join(app.repo, 'work.txt'), 'Uncommitted work');
  const next = join(app.directory, 'next'); await mkdir(next); project = await app.projects.save(app.org, { name: 'New codebase', codebase_path: next, revision: project.revision }, project.id);
  let result = await app.workspaces.list(app.org, project.id); assert.equal(result.workspaces[0].directory, next); assert.equal(result.workspaces.find(row => row.id === workspace.id).tasks[0].id, task.id);
  project = await app.projects.save(app.org, { name: 'No codebase', codebase_path: '', revision: project.revision }, project.id); result = await app.workspaces.list(app.org, project.id);
  assert.equal(result.workspaces.length, 1); assert.equal(result.workspaces[0].id, workspace.id); assert.equal(await readFile(join(app.repo, 'work.txt'), 'utf8'), 'Uncommitted work');
  assert.equal((await new TaskWorkspaces(app.tasks).prepare(app.org, task.id)).id, workspace.id);
  const empty = await app.projects.save(app.org, { name: 'Empty' }); const noWorkspace = await app.tasks.save(app.org, { project_id: empty.id, title: 'Needs code' });
  await assert.rejects(new TaskWorkspaces(app.tasks).prepare(app.org, noWorkspace.id), code('workspace_required'));
  await assert.rejects(new SkillProjects(new Skills(app.database, app.state)).scan(app.org, project.id), code('workspace_required'));
});

test('task runtime creates a sibling worktree once, pins base commit and reuses files through retries and Agent handoffs', async t => {
  const app = await fixture(t); await app.init(); const { agent, agents, runs } = await agentFixture(t, app); const project = await app.projects.save(app.org, { name: 'Runtime', codebase_path: app.repo });
  const task = await app.tasks.save(app.org, { project_id: project.id, title: 'Implement', agent_id: agent.id });
  const disabled = agents.saveIdentity(app.org, agent.id, { revision: agent.revisions.identity, enabled: false });
  await assert.rejects(runs.prepareForTask(app.org, task.id, 'disabled-run'), code('agent_disabled'));
  assert.equal(app.tasks.row(app.org, task.id).workspace_id, null); assert.equal(parseWorktrees(await app.runGit(app.repo, ['worktree', 'list', '--porcelain', '-z'])).length, 1);
  agents.saveIdentity(app.org, agent.id, { revision: disabled.revisions.identity, enabled: true });
  const first = await runs.prepareForTask(app.org, task.id, 'run-one');
  assert.equal(dirname(first.cwd), dirname(app.repo)); assert.notEqual(first.cwd, app.repo); assert.ok(basename(first.cwd).includes('orbit-')); assert.equal(first.snapshot.task_id, task.id);
  const workspace = app.workspaces.row(app.org, project.id, first.snapshot.workspace_id); assert.match(workspace.base_commit, /^[0-9a-f]{40,64}$/); assert.ok(workspace.task_branch.startsWith('orbit/')); assert.ok(workspace.last_used_at);
  await writeFile(join(first.cwd, 'changes.txt'), 'Keep this work');
  const secondAgent = await agents.create(app.org, { name: 'Approver', connector_id: agent.connector.id }); const current = app.tasks.get(app.org, task.id); await app.tasks.save(app.org, { revision: current.revision, agent_id: secondAgent.id }, task.id);
  const second = await runs.prepareForTask(app.org, task.id, 'run-two'); assert.equal(second.cwd, first.cwd); assert.notEqual(second.home, first.home); assert.equal(await readFile(join(second.cwd, 'changes.txt'), 'utf8'), 'Keep this work');
  await rm(first.cwd, { recursive: true, force: true }); await assert.rejects(runs.prepareForTask(app.org, task.id, 'missing-run'), code('workspace_unavailable'));
});

test('configured base commit, branch template and parent directory control actual worktrees and preserve existing bindings after edits', async t => {
  const app = await fixture(t); await app.init(); const initial = await app.runGit(app.repo, ['rev-parse', 'HEAD']);
  await app.runGit(app.repo, ['branch', 'starting-point']); await writeFile(join(app.repo, 'latest-only.txt'), 'Newer commit'); await app.runGit(app.repo, ['add', 'latest-only.txt']); await app.runGit(app.repo, ['commit', '-m', 'Newer']);
  const settings = { base_ref: 'starting-point', branch_template: 'tasks/{{issue.identifier}}-{{slug}}', parent_dir: '../configured-worktrees' };
  let project = await app.projects.save(app.org, { name: 'Configured', codebase_path: app.repo, worktree_settings: settings });
  assert.deepEqual(project.worktree_settings, settings); await assert.rejects(access(join(app.directory, 'configured-worktrees')));
  const task = await app.tasks.save(app.org, { project_id: project.id, title: 'Fix Login!' }); const preparer = new TaskWorkspaces(app.tasks); const workspace = await preparer.prepare(app.org, task.id);
  assert.equal(workspace.directory, join(app.directory, 'configured-worktrees', `tasks/${task.identifier}-fix-login`)); assert.equal(workspace.task_branch, `tasks/${task.identifier}-fix-login`); assert.equal(workspace.base_commit, initial);
  assert.equal(await app.runGit(workspace.directory, ['rev-parse', 'HEAD']), initial); await assert.rejects(access(join(workspace.directory, 'latest-only.txt'))); await writeFile(join(workspace.directory, 'changes.txt'), 'Keep edits');
  project = await app.projects.save(app.org, { name: project.name, codebase_path: app.repo, revision: project.revision, worktree_settings: { base_ref: 'HEAD', branch_template: 'next/{{issue.id}}', parent_dir: join(app.directory, 'next-worktrees') } }, project.id);
  assert.equal((await preparer.prepare(app.org, task.id)).id, workspace.id); assert.equal(await readFile(join(workspace.directory, 'changes.txt'), 'utf8'), 'Keep edits');
  const nextTask = await app.tasks.save(app.org, { project_id: project.id, title: 'Next task' }); const next = await preparer.prepare(app.org, nextTask.id); assert.equal(next.task_branch, `next/${nextTask.id}`); assert.equal(dirname(dirname(next.directory)), join(app.directory, 'next-worktrees')); assert.notEqual(next.base_commit, initial);
  project = await app.projects.save(app.org, { name: project.name, codebase_path: app.repo, revision: project.revision, worktree_settings: { base_ref: '', branch_template: '', parent_dir: '' } }, project.id);
  assert.deepEqual(project.worktree_settings, { base_ref: null, branch_template: null, parent_dir: null });
  const thirdTask = await app.tasks.save(app.org, { project_id: project.id, title: 'Default task' }); const third = await preparer.prepare(app.org, thirdTask.id); assert.equal(third.directory, join(app.directory, `${basename(app.repo)}-orbit-${app.org.slice(0, 8)}-${thirdTask.identifier}`));
  const reopened = openDatabase(app.state); try { assert.deepEqual(new Projects(reopened).get(app.org, project.id).worktree_settings, project.worktree_settings); } finally { reopened.close(); }
});

test('invalid refs, templates and parent paths fail atomically, and a ref deleted after saving never falls back to HEAD', async t => {
  const app = await fixture(t); await app.init(); let project = await app.projects.save(app.org, { name: 'Validation', codebase_path: app.repo });
  const file = join(app.directory, 'file'); await writeFile(file, 'Not a directory'); const alias = join(app.directory, 'alias-into-repository'); await symlink(app.repo, alias, 'dir');
  const invalid = [
    [{ base_ref: 'missing-branch' }, 'invalid_base_ref'], [{ base_ref: '--all' }, 'invalid_base_ref'],
    [{ branch_template: 'feature/{{unknown}}' }, 'invalid_branch_template'], [{ branch_template: '../{{slug}}' }, 'invalid_branch_template'], [{ branch_template: '--bad' }, 'invalid_branch_template'], [{ branch_template: 'feature/{{slug}' }, 'invalid_branch_template'], [{ branch_template: 'feature/has space' }, 'invalid_branch_template'],
    [{ parent_dir: '.' }, 'invalid_worktree_parent'], [{ parent_dir: '.paperclip/worktrees' }, 'invalid_worktree_parent'], [{ parent_dir: alias }, 'invalid_worktree_parent'], [{ parent_dir: file }, 'invalid_worktree_parent'],
    [{ provision_command: 'echo unsupported' }, 'invalid_worktree_settings'],
  ];
  for (const [settings, error] of invalid) await assert.rejects(app.projects.save(app.org, { name: 'Must not change', codebase_path: app.repo, revision: project.revision, worktree_settings: settings }, project.id), code(error));
  assert.equal(app.projects.get(app.org, project.id).name, 'Validation'); assert.equal(app.projects.get(app.org, project.id).revision, 1);
  await app.runGit(app.repo, ['branch', 'temporary-base']); project = await app.projects.save(app.org, { name: project.name, codebase_path: app.repo, revision: project.revision, worktree_settings: { base_ref: 'temporary-base' } }, project.id); await app.runGit(app.repo, ['branch', '-D', 'temporary-base']);
  const task = await app.tasks.save(app.org, { project_id: project.id, title: 'Deleted base' }); await assert.rejects(new TaskWorkspaces(app.tasks).prepare(app.org, task.id), code('invalid_base_ref'));
  assert.equal(app.tasks.row(app.org, task.id).workspace_id, null); assert.equal(parseWorktrees(await app.runGit(app.repo, ['worktree', 'list', '--porcelain', '-z'])).length, 1);
});

test('custom worktree paths cannot escape their configured parent through nested symlinks', async t => {
  const app = await fixture(t); await app.init(); const parent = join(app.directory, 'parent'); const outside = join(app.directory, 'outside');
  const project = await app.projects.save(app.org, { name: 'Contained', codebase_path: app.repo, worktree_settings: { branch_template: 'feature/{{issue.identifier}}', parent_dir: parent } });
  await mkdir(parent); await mkdir(outside); await symlink(outside, join(parent, 'feature'), 'dir');
  const task = await app.tasks.save(app.org, { project_id: project.id, title: 'Contained task' }); await assert.rejects(new TaskWorkspaces(app.tasks).prepare(app.org, task.id), code('invalid_worktree_parent'));
  await assert.rejects(access(join(outside, task.identifier))); assert.equal(app.tasks.row(app.org, task.id).workspace_id, null);
});

test('Projects HTTP APIs enforce login, origin and org boundaries and expose directory validation and real task/workspace links', async t => {
  const app = await fixture(t); await app.init(); await setAccount(app.database, 'project-user', 'test-password');
  const server = createOrbitServer(configuration({ ORBIT_STATE_DIR: app.state }), app.database); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(() => new Promise(resolve => { server.close(resolve); server.closeAllConnections(); })); const root = `http://127.0.0.1:${server.address().port}`;
  assert.equal((await fetch(`${root}/api/orgs/${app.org}/projects`)).status, 401);
  const login = await fetch(`${root}/api/auth/login`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'project-user', password: 'test-password' }) }); const cookie = login.headers.get('set-cookie').split(';')[0];
  const request = (path, method = 'GET', value, extra = {}) => fetch(`${root}/api/orgs/${app.org}${path}`, { method, headers: { Cookie: cookie, 'Content-Type': 'application/json', ...extra }, ...(value ? { body: JSON.stringify(value) } : {}) });
  assert.equal((await request('/projects', 'POST', { name: 'Blocked' }, { Origin: 'https://bad.example' })).status, 403);
  const settings = { base_ref: 'main', branch_template: 'http-{{issue.identifier}}', parent_dir: '../http-worktrees' };
  const project = await (await request('/projects', 'POST', { name: 'HTTP', codebase_path: app.repo, worktree_settings: settings })).json(); assert.ok(project.id); assert.deepEqual(project.worktree_settings, settings);
  assert.deepEqual((await (await request(`/projects/${project.id}`)).json()).worktree_settings, settings);
  const workspace = (await (await request(`/projects/${project.id}/workspaces`)).json()).workspaces[0];
  const created = await request(`/projects/${project.id}/tasks`, 'POST', { title: 'HTTP task', workspace_id: workspace.id }); assert.equal(created.status, 201); const task = await created.json();
  assert.equal((await (await request(`/projects/${project.id}/tasks`)).json())[0].id, task.id); assert.equal((await request(`/tasks/${task.id}`)).status, 200);
  assert.equal((await fetch(`${root}/api/orgs/${app.other}/projects/${project.id}`, { headers: { Cookie: cookie } })).status, 404);
  assert.equal((await fetch(`${root}/api/orgs/${app.other}/tasks/${task.id}`, { headers: { Cookie: cookie } })).status, 404);
  assert.equal((await request('/projects/inspect', 'POST', { directory: 'relative' })).status, 422);
  assert.equal((await request(`/projects/directories?path=${encodeURIComponent(app.directory)}`)).status, 200);
  assert.equal((await request(`/projects/${project.id}/workspaces`, 'POST', {})).status, 405);
});
