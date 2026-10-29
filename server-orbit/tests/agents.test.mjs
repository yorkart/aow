import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { mkdir, mkdtemp, readFile, rm, stat, unlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { test } from 'node:test';
import { setAccount } from '../dist/auth.js';
import { configuration } from '../dist/config.js';
import { openDatabase } from '../dist/database.js';
import { Connectors } from '../dist/connectors/service.js';
import { Agents } from '../dist/agents/service.js';
import { AgentRuns } from '../dist/agents/runtime.js';
import { Skills } from '../dist/skills/service.js';
import { createOrbitServer } from '../dist/http.js';

const code = expected => error => error.code === expected;
const md = (path, content) => ({ path, kind: 'file', encoding: 'utf8', content, executable: false });
async function fixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'orbit-agents-')); const state = join(directory, 'state'); const home = join(directory, 'operator'); const bin = join(directory, 'bin'); const project = join(directory, 'project');
  for (const path of [bin, project, join(home, '.codex'), join(home, '.claude'), join(home, '.gemini')]) await mkdir(path, { recursive: true });
  const script = await readFile(new URL('./fixtures/connector-cli.mjs', import.meta.url), 'utf8');
  for (const name of ['codex', 'claude', 'gemini']) await writeFile(join(bin, name), `#!${process.execPath}\n${script}`, { mode: 0o700 });
  await writeFile(join(home, '.codex/auth.json'), JSON.stringify({ OPENAI_API_KEY: 'local-identity-first' }));
  await writeFile(join(home, '.codex/models_cache.json'), JSON.stringify({ models: [{ slug: 'gpt-5.4', supported_reasoning_levels: [{ effort: 'low' }, { effort: 'high' }] }] }));
  await writeFile(join(home, '.claude/.credentials.json'), JSON.stringify({ claudeAiOauth: { accessToken: 'test-claude-local' } }));
  await writeFile(join(home, '.gemini/oauth_creds.json'), JSON.stringify({ access_token: 'test-gemini-local' }));
  await writeFile(join(project, 'AGENTS.md'), 'project-instructions-stay-here');
  await writeFile(join(home, '.codex/AGENTS.md'), 'operator-instructions-stay-here');
  const database = openDatabase(state); const org = randomUUID(); const other = randomUUID(); const projectId = randomUUID();
  for (const id of [org, other]) database.prepare('INSERT INTO orgs VALUES (?, ?, ?)').run(id, id, new Date().toISOString());
  database.prepare('INSERT INTO projects VALUES (?, ?, ?, ?)').run(projectId, org, 'Project', project);
  const options = { localHome: home, env: { ...process.env, PATH: bin + delimiter + process.env.PATH, CODEX_HOME: join(home, '.codex'), CLAUDE_CONFIG_DIR: join(home, '.claude'), GEMINI_CLI_HOME: home, OPENAI_API_KEY: 'inherited-must-not-leak' }, fetch: async () => Response.json({ data: [{ id: 'gpt-5.4' }, { id: 'claude-sonnet-4-6' }, { id: 'deepseek-chat' }] }) };
  const connectors = new Connectors(database, state, options); const skills = new Skills(database, state); const agents = new Agents(database, state, connectors, skills); const runs = new AgentRuns(agents);
  const connector = await connectors.save(org, { name: 'Local Codex', provider: 'openai', executor: 'codex', auth_method: 'local', account_note: '', connect: true });
  const create = (name = 'Reviewer') => agents.create(org, { name, role: 'Review', description: 'Review changes', connector_id: connector.id });
  t.after(async () => { await connectors.close(); database.close(); await rm(directory, { recursive: true, force: true }); });
  return { directory, state, home, bin, project, projectId, database, org, other, options, connectors, skills, agents, runs, connector, create };
}

test('agents are org-scoped, require a usable Connector, create independent instructions and expose real references', async t => {
  const app = await fixture(t); const first = await app.create(); const second = await app.create('Approver');
  assert.equal(first.status, 'idle'); assert.deepEqual(first.recent_tasks, []); assert.equal(first.last_run_at, null);
  assert.deepEqual(app.agents.list(app.other), []); assert.throws(() => app.agents.get(app.other, first.id), code('agent_not_found'));
  await assert.rejects(app.agents.create(app.other, { name: 'Cross org', connector_id: app.connector.id }), code('connector_not_found'));
  assert.equal(app.connectors.get(app.org, app.connector.id).agent_count, 2);
  assert.throws(() => app.connectors.remove(app.org, app.connector.id, app.connector.revision), code('connector_in_use'));
  const instructions = await app.agents.instructionFiles(app.org, first.id); assert.match(instructions.files[0].content, /Reviewer/);
  await app.agents.saveInstructions(app.org, first.id, { base_version_id: instructions.version_id, files: [md('AGENTS.md', 'Reviewer only')] });
  assert.match((await app.agents.instructionFiles(app.org, second.id)).files[0].content, /Approver/);
  const disabled = app.connectors.enable(app.org, app.connector.id, { revision: app.connector.revision, enabled: false });
  await assert.rejects(app.create('Unavailable'), code('connector_unavailable'));
  assert.equal(app.agents.get(app.org, first.id).status, 'error'); assert.match(app.agents.get(app.org, first.id).status_message, /Connector/);
  assert.equal(disabled.status, 'disabled');
});

test('each configuration section has independent optimistic revisions, and instruction validation is atomic', async t => {
  const app = await fixture(t); const first = await app.create();
  app.agents.saveIdentity(app.org, first.id, { revision: first.revisions.identity, name: 'Reviewer updated' });
  const configured = await app.agents.saveRuntime(app.org, first.id, { revision: first.revisions.runtime, connector_id: app.connector.id, runtime: { model: 'gpt-5.4', reasoning_effort: 'high', timeout_seconds: 600 } });
  assert.equal(configured.name, 'Reviewer updated'); assert.equal(configured.runtime.model, 'gpt-5.4');
  assert.throws(() => app.agents.saveIdentity(app.org, first.id, { revision: first.revisions.identity, name: 'Stale' }), code('agent_conflict'));
  const instructions = await app.agents.instructionFiles(app.org, first.id);
  for (const files of [[md('guide.md', 'No entry')], [md('AGENTS.md', 'Entry'), md('../escape.md', 'Bad')], [md('AGENTS.md', 'Entry'), md('script.sh', 'Bad')]]) await assert.rejects(app.agents.saveInstructions(app.org, first.id, { base_version_id: instructions.version_id, files }));
  assert.equal((await app.agents.instructionFiles(app.org, first.id)).version_id, instructions.version_id);
  const saved = await app.agents.saveInstructions(app.org, first.id, { base_version_id: instructions.version_id, files: [md('AGENTS.md', '[Guide](checks/guide.md)'), md('checks/guide.md', 'Check output')] });
  await assert.rejects(app.agents.saveInstructions(app.org, first.id, { base_version_id: instructions.version_id, files: [md('AGENTS.md', 'Stale')] }), code('agent_conflict'));
  assert.notEqual(saved.instructions_version_id, instructions.version_id);
  assert.match((await app.agents.instructions.read(app.org, first.id, instructions.version_id))[0].content, /Reviewer/);
});

test('Agent secrets are encrypted, never echoed, preserve and replace values by scoped reference, and cannot override managed fields', async t => {
  const app = await fixture(t); let agent = await app.create(); const second = await app.create('Other');
  agent = app.agents.saveVariables(app.org, agent.id, { revision: agent.revisions.variables, variables: [{ name: 'REVIEW_LANGUAGE', kind: 'variable', value: 'zh-CN' }, { name: 'SERVICE_TOKEN', kind: 'secret', value: 'first-private-secret' }] });
  assert.equal(JSON.stringify(agent).includes('first-private-secret'), false); assert.equal(app.agents.variables.environment(app.org, agent.id).SERVICE_TOKEN, 'first-private-secret');
  assert.equal(JSON.stringify(app.database.prepare('SELECT * FROM agent_secrets').all()).includes('first-private-secret'), false);
  assert.equal((await stat(join(app.state, 'credentials.key'))).mode & 0o777, 0o600);
  const token = agent.variables.find(row => row.kind === 'secret'); assert.equal(token.value, undefined); assert.equal(token.configured, true);
  agent = app.agents.saveVariables(app.org, agent.id, { revision: agent.revisions.variables, variables: [{ ...token, name: 'RENAMED_TOKEN' }] });
  assert.equal(app.agents.variables.environment(app.org, agent.id).RENAMED_TOKEN, 'first-private-secret');
  assert.throws(() => app.agents.saveVariables(app.org, second.id, { revision: second.revisions.variables, variables: [token] }), code('invalid_variables'));
  for (const name of ['HOME', 'PATH', 'OPENAI_API_KEY', 'CLAUDE_CODE_OAUTH_TOKEN', 'CODEX_HOME', 'ANTHROPIC_MODEL', 'NODE_OPTIONS']) assert.throws(() => app.agents.saveVariables(app.org, agent.id, { revision: agent.revisions.variables, variables: [{ name, kind: 'variable', value: 'blocked' }] }), code('managed_variable'));
  assert.throws(() => app.agents.saveVariables(app.org, agent.id, { revision: agent.revisions.variables, variables: [{ name: 'NEW_TOKEN', kind: 'secret', value: 'must-rollback' }, { name: 'NEW_TOKEN', kind: 'variable', value: 'duplicate' }] }), code('invalid_variables'));
  assert.equal(app.database.prepare('SELECT count(*) AS count FROM agent_secrets').get().count, 1);
  agent = app.agents.saveVariables(app.org, agent.id, { revision: agent.revisions.variables, variables: [{ ...agent.variables[0], value: 'replacement-private-secret' }] });
  assert.equal(app.agents.variables.environment(app.org, agent.id).RENAMED_TOKEN, 'replacement-private-secret');
  await unlink(join(app.state, 'credentials.key'));
  assert.throws(() => app.agents.variables.environment(app.org, agent.id), code('credential_store_unavailable'));
});

test('runtime capabilities follow Connector and native model capabilities, reject incompatible configuration and check without inference', async t => {
  const app = await fixture(t); const agent = await app.create();
  const options = await app.agents.runtimeOptions(app.org, app.connector.id);
  assert.deepEqual(options.models.find(model => model.id === 'gpt-5.4').reasoning_efforts, ['low', 'high']);
  await assert.rejects(app.agents.saveRuntime(app.org, agent.id, { revision: 1, connector_id: app.connector.id, runtime: { model: 'not-supported' } }), code('invalid_runtime'));
  await assert.rejects(app.agents.saveRuntime(app.org, agent.id, { revision: 1, connector_id: app.connector.id, runtime: { model: 'gpt-5.4', reasoning_effort: 'invented' } }), code('invalid_runtime'));
  const claude = await app.connectors.save(app.org, { name: 'Claude', provider: 'anthropic', executor: 'claude', auth_method: 'local', account_note: '', connect: true });
  await assert.rejects(app.agents.saveRuntime(app.org, agent.id, { revision: 1, connector_id: claude.id, runtime: { model: 'gpt-5.4' } }), code('invalid_runtime'));
  assert.equal((await app.agents.checkRuntime(app.org, agent.id, {})).status, 'available');
  await unlink(join(app.bin, 'codex'));
  app.options.env.PATH = app.bin;
  assert.equal((await app.agents.checkRuntime(app.org, agent.id, {})).status, 'error');
  assert.equal(app.agents.get(app.org, agent.id).status, 'error');
});

test('run snapshots isolate identity, instructions, selected Skill versions, model and secrets while preserving project and operator files', async t => {
  const app = await fixture(t); let agent = await app.create();
  const skill = await app.skills.create(app.org, { name: 'review', description: 'Review files', instructions: 'Skill original' });
  agent = app.agents.saveSkills(app.org, agent.id, { revision: 1, skill_ids: [skill.id] });
  agent = app.agents.saveVariables(app.org, agent.id, { revision: 1, variables: [{ name: 'TOKEN', kind: 'secret', value: 'run-secret-original' }, { name: 'LANGUAGE', kind: 'variable', value: 'zh-CN' }] });
  const first = await app.runs.prepare(app.org, agent.id, 'first-run', app.projectId); app.runs.start(app.org, agent.id, 'first-run');
  assert.equal(app.agents.get(app.org, agent.id).status, 'running'); assert.equal(first.cwd, app.project); assert.equal(first.env.TOKEN, 'run-secret-original');
  assert.equal(first.env.HOME, first.home); assert.notEqual(first.home, app.home); assert.equal(first.env.OPENAI_API_KEY, undefined);
  const entry = await readFile(join(first.home, '.codex/AGENTS.md'), 'utf8'); const skillPath = join(first.home, '.agents/skills/review/SKILL.md'); const skillEntry = await readFile(skillPath, 'utf8');
  agent = await app.agents.saveInstructions(app.org, agent.id, { base_version_id: agent.instructions_version_id, files: [md('AGENTS.md', 'Instruction replacement')] });
  await app.skills.save(app.org, skill.id, { base_version_id: skill.current_version_id, files: skill.files.map(file => file.path === 'SKILL.md' ? { ...file, content: file.content.replace('Skill original', 'Skill replacement') } : file) });
  agent = app.agents.saveSkills(app.org, agent.id, { revision: agent.revisions.skills, skill_ids: [] });
  agent = app.agents.saveVariables(app.org, agent.id, { revision: agent.revisions.variables, variables: [{ ...agent.variables.find(row => row.name === 'TOKEN'), value: 'run-secret-new' }] });
  await writeFile(join(app.home, '.codex/auth.json'), JSON.stringify({ OPENAI_API_KEY: 'local-identity-second' }));
  const second = await app.runs.prepare(app.org, agent.id, 'second-run', app.projectId);
  assert.equal(first.env.TOKEN, 'run-secret-original'); assert.equal(second.env.TOKEN, 'run-secret-new');
  assert.equal(await readFile(join(first.home, '.codex/AGENTS.md'), 'utf8'), entry); assert.equal(await readFile(skillPath, 'utf8'), skillEntry);
  assert.match(await readFile(join(second.home, '.codex/AGENTS.md'), 'utf8'), /Instruction replacement/);
  await assert.rejects(readFile(join(second.home, '.agents/skills/review/SKILL.md')));
  assert.equal(JSON.parse(await readFile(join(first.home, '.codex/auth.json'), 'utf8')).OPENAI_API_KEY, 'local-identity-first');
  assert.equal(JSON.parse(await readFile(join(second.home, '.codex/auth.json'), 'utf8')).OPENAI_API_KEY, 'local-identity-second');
  assert.equal(await readFile(join(app.project, 'AGENTS.md'), 'utf8'), 'project-instructions-stay-here'); assert.equal(await readFile(join(app.home, '.codex/AGENTS.md'), 'utf8'), 'operator-instructions-stay-here');
  assert.equal((await readFile(join(second.home, '../snapshot.json'), 'utf8')).includes('run-secret-new'), false);
  assert.equal(JSON.stringify(app.database.prepare('SELECT snapshot_json FROM agent_runs').all()).includes('run-secret'), false);
  await assert.rejects(app.runs.prepare(app.other, agent.id, 'cross-org', app.projectId), code('agent_not_found'));
  await assert.rejects(app.runs.prepare(app.org, agent.id, 'first-run', app.projectId), code('execution_exists'));
  app.agents.saveIdentity(app.org, agent.id, { revision: agent.revisions.identity, enabled: false });
  await assert.rejects(app.runs.prepare(app.org, agent.id, 'blocked', app.projectId), code('agent_disabled'));
  assert.throws(() => app.runs.start(app.org, agent.id, 'second-run'), code('agent_disabled'));
  app.runs.finish(app.org, agent.id, 'first-run', true); assert.equal(app.agents.get(app.org, agent.id).running_count, 0);
});

test('Claude and Gemini runtime snapshots load role instructions and complete Skills in native isolated directories', async t => {
  const app = await fixture(t); const skill = await app.skills.create(app.org, { name: 'check', description: 'Check', instructions: 'Complete skill' });
  for (const [provider, executor, native] of [['anthropic', 'claude', 'CLAUDE.md'], ['gemini', 'gemini', 'GEMINI.md']]) {
    const connector = await app.connectors.save(app.org, { name: executor, provider, executor, auth_method: 'local', account_note: '', connect: true });
    let agent = await app.agents.create(app.org, { name: executor, connector_id: connector.id }); agent = app.agents.saveSkills(app.org, agent.id, { revision: 1, skill_ids: [skill.id] });
    const run = await app.runs.prepare(app.org, agent.id, 'native-run', app.projectId);
    assert.match(await readFile(join(run.home, `.${executor}`, native), 'utf8'), /@orbit-instructions\/AGENTS.md/);
    assert.match(await readFile(join(run.home, `.${executor}`, 'skills/check/SKILL.md'), 'utf8'), /Complete skill/);
  }
});

test('a failed run preparation removes its pinned Skill records so a retry cannot reuse an outdated selection', async t => {
  const app = await fixture(t); let agent = await app.create();
  const skill = await app.skills.create(app.org, { name: 'retry-check', description: 'Check', instructions: 'Pinned before failure' });
  agent = app.agents.saveSkills(app.org, agent.id, { revision: 1, skill_ids: [skill.id] });
  const original = app.connectors.prepareExecution.bind(app.connectors);
  app.connectors.prepareExecution = async (...args) => { const execution = await original(...args); await writeFile(join(execution.home, '../snapshot.json'), 'occupy snapshot target'); return execution; };
  await assert.rejects(app.runs.prepare(app.org, agent.id, 'retry-run', app.projectId));
  assert.equal(app.database.prepare('SELECT count(*) AS count FROM skill_runs').get().count, 0);
  assert.equal(app.database.prepare('SELECT count(*) AS count FROM agent_runs').get().count, 0);
  app.connectors.prepareExecution = original;
  agent = app.agents.saveSkills(app.org, agent.id, { revision: agent.revisions.skills, skill_ids: [] });
  const run = await app.runs.prepare(app.org, agent.id, 'retry-run', app.projectId);
  assert.deepEqual(run.snapshot.skills, []);
  await assert.rejects(readFile(join(run.home, '.agents/skills/retry-check/SKILL.md')));
});

test('Agent HTTP routes require authentication, protect origins, enforce org boundaries and never expose decrypted secrets', async t => {
  const app = await fixture(t); await setAccount(app.database, 'agent-user', 'test-password');
  const server = createOrbitServer(configuration({ ORBIT_STATE_DIR: app.state }), app.database, app.options); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(() => new Promise(resolve => { server.close(resolve); server.closeAllConnections(); }));
  const root = `http://127.0.0.1:${server.address().port}`; const path = `/api/orgs/${app.org}/agents`;
  assert.equal((await fetch(root + path)).status, 401);
  const login = await fetch(root + '/api/auth/login', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username: 'agent-user', password: 'test-password' }) }); const cookie = login.headers.get('set-cookie').split(';')[0];
  const request = (suffix = '', method = 'GET', value, extra = {}) => fetch(root + path + suffix, { method, headers: { Cookie: cookie, 'Content-Type': 'application/json', ...extra }, ...(value ? { body: JSON.stringify(value) } : {}) });
  assert.equal((await request('', 'POST', { name: 'Blocked' }, { Origin: 'https://invalid.example' })).status, 403);
  const created = await request('', 'POST', { name: 'HTTP Agent', connector_id: app.connector.id }); assert.equal(created.status, 201); const agent = await created.json();
  const saved = await request(`/${agent.id}/variables`, 'PUT', { revision: 1, variables: [{ name: 'TOKEN', kind: 'secret', value: 'http-private-value' }] }); assert.equal(saved.status, 200); assert.equal((await saved.text()).includes('http-private-value'), false);
  assert.equal((await (await request(`/${agent.id}`)).text()).includes('http-private-value'), false);
  assert.equal((await fetch(`${root}/api/orgs/${app.other}/agents/${agent.id}`, { headers: { Cookie: cookie } })).status, 404);
  assert.equal((await request(`/${agent.id}/runtime`, 'POST', {})).status, 405);
  assert.equal((await request(`/${agent.id}/instructions`)).status, 200);
});
