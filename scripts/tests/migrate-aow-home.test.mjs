import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import test from 'node:test';

const script = resolve('scripts/migrate-aow-home.py');
function fixture(t, homeName) {
  const temporary = realpathSync(mkdtempSync(join(tmpdir(), 'aow-migrate-')));
  const home = homeName ? join(temporary, homeName) : temporary;
  t.after(() => rmSync(temporary, { recursive: true, force: true }));
  const state = join(home, '.local/state/aow');
  const config = join(state, 'config-repo/selected');
  const destination = join(home, '.aow');
  function write(path, content) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  write(join(state, 'config.toml'), `config-repo = ${JSON.stringify(dirname(config))}\nconfig-id = 'selected'\n`);
  write(join(config, 'aow-projects.json'), JSON.stringify({ version: 1, items: [{ registered_path: join(state, 'repos/global') }] }));
  const jsonc = '// keep comments\n{"agent_servers": {},}\n';
  write(join(config, 'acp/settings.json'), jsonc);
  write(join(config, 'inbox/requirements/active/item/comments.jsonl'), '{"markdown":"keep text"}\n');
  write(join(state, 'inbox/selected/executions.json'), '[]');
  write(join(state, 'credentials.json'), '{"password_hash":"private"}');
  write(join(state, 'terminals.json'), '{}');
  write(join(state, 'acp/threads.sqlite'), Buffer.from([0, 1, 2, 255]));
  write(join(state, 'acp/agents/demo/bin/run'), 'executable');
  write(join(state, 'automations/runs/task/run/result.json'), '{}');
  write(join(state, 'data/notes/project/note.md'), 'keep existing notes');
  write(join(config, 'automations/tasks/task.json'), '{"id":"task"}');
  write(join(state, 'config-repo/.git/HEAD'), 'do not copy');
  write(join(state, 'config-repo/unselected/aow-settings.json'), 'do not select');
  write(join(destination, 'skills/custom/SKILL.md'), 'existing skill');
  write(join(home, '.config/aow/server.env'), `# preserve\nAOW_STATE_DIR=${state}\nXDG_STATE_HOME=/old\nHTTP_PROXY=http://localhost:8080\n`);
  const run = (...args) => spawnSync('python3', [script, '--home', home, '--state-dir', state, ...args], { encoding: 'utf8' });
  return { home, state, config, destination, jsonc, write, run };
}

test('copies the selected configuration, data and cache without changing sources', t => {
  const f = fixture(t);
  const before = readFileSync(join(f.config, 'aow-projects.json'));
  const result = f.run();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(readFileSync(join(f.destination, 'aow-acp.json'), 'utf8'), f.jsonc);
  assert.deepEqual(readFileSync(join(f.destination, 'data/acp/threads.sqlite')), Buffer.from([0, 1, 2, 255]));
  assert.equal(readFileSync(join(f.destination, 'cache/acp/agents/demo/bin/run'), 'utf8'), 'executable');
  assert.equal(readFileSync(join(f.destination, 'skills/custom/SKILL.md'), 'utf8'), 'existing skill');
  assert.equal(readFileSync(join(f.destination, 'data/notes/project/note.md'), 'utf8'), 'keep existing notes');
  assert.equal(JSON.parse(readFileSync(join(f.destination, 'aow-projects.json'))).items[0].registered_path, join(f.destination, 'data/repos/global'));
  assert.deepEqual(readFileSync(join(f.config, 'aow-projects.json')), before);
  assert.equal(lstatSync(join(f.destination, 'credentials.json')).mode & 0o777, 0o600);
  for (const path of ['state/terminals.json', 'state/inbox/executions.json', 'data/inbox/requirements/active/item/comments.jsonl', 'automations/tasks/task.json', 'state/automations/runs/task/run/result.json']) {
    assert.ok(existsSync(join(f.destination, path)), path);
  }
  for (const path of ['.git', 'config-repo', 'config.toml', 'acp/settings.json', 'aow-settings.json']) assert.equal(existsSync(join(f.destination, path)), false, path);
  assert.equal(readFileSync(join(f.destination, 'server.env'), 'utf8'), `# preserve\nAOW_STATE_DIR=${JSON.stringify(f.destination)}\nHTTP_PROXY=http://localhost:8080\n`);
  const repeated = f.run();
  assert.equal(repeated.status, 0, repeated.stderr);
  assert.match(repeated.stdout, /0 to copy/);
});

test('dry run and conflicting destinations leave the destination unchanged', t => {
  const f = fixture(t);
  assert.equal(f.run('--dry-run').status, 0);
  assert.equal(existsSync(join(f.destination, 'aow-acp.json')), false);
  f.write(join(f.destination, 'aow-acp.json'), 'existing different configuration');
  const result = f.run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Destination conflict/);
  assert.equal(existsSync(join(f.destination, 'credentials.json')), false);
  assert.equal(readFileSync(join(f.destination, 'aow-acp.json'), 'utf8'), 'existing different configuration');
});

test('preserves package symlinks and refuses destination symlink traversal', t => {
  const f = fixture(t);
  const source = join(f.state, 'acp/agents/demo/bin/link');
  symlinkSync('run', source);
  assert.equal(f.run().status, 0);
  assert.equal(readlinkSync(join(f.destination, 'cache/acp/agents/demo/bin/link')), 'run');
  rmSync(join(f.destination, 'data'), { recursive: true });
  const outside = join(f.home, 'outside');
  mkdirSync(outside);
  symlinkSync(outside, join(f.destination, 'data'));
  const result = f.run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /not a regular directory/);
  assert.equal(existsSync(join(outside, 'acp')), false);
});

test('relocates ACP JSONC paths while preserving comments, and creates private directories', t => {
  const f = fixture(t);
  const executable = join(f.state, 'acp/agents/demo/bin/run');
  const source = `// ${executable} remains a comment\n{"agent_servers":{"demo":{"command":${JSON.stringify(executable)},}},}\n`;
  f.write(join(f.config, 'acp/settings.json'), source);
  const destination = join(f.home, 'new home');
  const result = f.run('--destination', destination);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(join(destination, 'aow-acp.json'), 'utf8'), source.replace(JSON.stringify(executable), JSON.stringify(join(destination, 'cache/acp/agents/demo/bin/run'))));
  for (const path of ['', 'data', 'data/acp', 'state', 'state/automations', 'cache']) {
    assert.equal(lstatSync(join(destination, path)).mode & 0o777, 0o700, path);
  }
  assert.equal(readFileSync(join(f.config, 'acp/settings.json'), 'utf8'), source);
});

test('copies committed SQLite WAL data and relocates ACP workspace metadata without editing transcripts or sources', t => {
  const f = fixture(t);
  const source = join(f.state, 'acp/threads.sqlite');
  const cwd = join(f.state, 'repos/global');
  rmSync(source);
  const prepared = spawnSync('python3', ['-c', `
import json, os, sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.execute('PRAGMA journal_mode=WAL')
db.execute('PRAGMA wal_autocheckpoint=0')
db.execute('CREATE TABLE sidebar_threads(thread_id TEXT PRIMARY KEY, folder_paths TEXT)')
db.execute('CREATE TABLE acp_thread_snapshots(thread_id TEXT PRIMARY KEY, data_type TEXT, data BLOB)')
db.execute('INSERT INTO sidebar_threads VALUES (?, ?)', ('thread', json.dumps([sys.argv[2]])))
db.execute('INSERT INTO acp_thread_snapshots VALUES (?, ?, ?)', ('thread', 'json', json.dumps({'cwd': sys.argv[2], 'message': sys.argv[2]}).encode()))
db.commit()
os._exit(0)
`, source, cwd], { encoding: 'utf8' });
  assert.equal(prepared.status, 0, prepared.stderr);
  const suffixes = ['', '-wal', '-shm'];
  const originals = suffixes.map(suffix => readFileSync(source + suffix));
  assert.equal(f.run('--dry-run').status, 0);
  assert.equal(existsSync(join(f.destination, 'data/acp/threads.sqlite')), false);
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  const target = join(f.destination, 'data/acp/threads.sqlite');
  const read = spawnSync('python3', ['-c', `
import json, sqlite3, sys
db = sqlite3.connect(sys.argv[1])
print(json.dumps([json.loads(db.execute('SELECT folder_paths FROM sidebar_threads').fetchone()[0]), json.loads(db.execute('SELECT data FROM acp_thread_snapshots').fetchone()[0])]))
`, target], { encoding: 'utf8' });
  assert.equal(read.status, 0, read.stderr);
  const relocated = join(f.destination, 'data/repos/global');
  assert.deepEqual(JSON.parse(read.stdout), [[relocated], { cwd: relocated, message: cwd }]);
  for (const [index, suffix] of suffixes.entries()) assert.deepEqual(readFileSync(source + suffix), originals[index]);
  assert.equal(existsSync(target + '-wal'), false);
  assert.equal(existsSync(target + '-shm'), false);
  const repeated = f.run();
  assert.equal(repeated.status, 0, repeated.stderr);
  assert.match(repeated.stdout, /0 to copy/);
});

test('relocates copied Git worktree pointers and reports external scheduled task registrations', t => {
  const f = fixture(t);
  const gitdir = join(f.state, 'repos/global/.git/worktrees/work');
  f.write(join(f.state, 'repos/work/.git'), `gitdir: ${gitdir}\n`);
  f.write(join(gitdir, 'gitdir'), join(f.state, 'repos/work/.git') + '\n');
  const registration = join(f.home, 'Library/LaunchAgents/org.aow.automation.example.plist');
  f.write(registration, 'existing registration');
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(join(f.destination, 'data/repos/work/.git'), 'utf8'), `gitdir: ${join(f.destination, 'data/repos/global/.git/worktrees/work')}\n`);
  assert.equal(readFileSync(join(f.destination, 'data/repos/global/.git/worktrees/work/gitdir'), 'utf8'), join(f.destination, 'data/repos/work/.git') + '\n');
  assert.match(result.stdout, /1 existing OS automation registrations/);
  assert.equal(readFileSync(registration, 'utf8'), 'existing registration');
});

test('preserves user JSON, task prompts and completed results while relocating known metadata paths', t => {
  const f = fixture(t);
  const repository = join(f.state, 'repos/global');
  const content = JSON.stringify({ cwd: repository, example_path: repository, prompt: repository });
  f.write(join(repository, 'example.json'), content);
  const git = (cwd, ...args) => {
    const result = spawnSync('git', ['-C', cwd, ...args], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  git(repository, 'init', '--template=');
  git(repository, 'add', 'example.json');
  git(repository, '-c', 'user.name=Migration Fixture', '-c', 'user.email=fixture@example.test', '-c', 'commit.gpgsign=false', 'commit', '-m', 'fixture');
  const untouched = [
    ['data/notes/project/example.json', join(f.state, 'data/notes/project/example.json')],
    ['custom.json', join(f.config, 'custom.json')],
    ['cache/acp/agents/demo/package.json', join(f.state, 'acp/agents/demo/package.json')],
    ['state/automations/runs/task/run/result.json', join(f.state, 'automations/runs/task/run/result.json')],
  ];
  for (const [, path] of untouched) f.write(path, content);
  const task = { repository_path: repository, workspace_path: repository, prompt: repository, name: repository, launch: { executable: repository + '/run', args: [repository], environment: { AGENT_HOME: repository } } };
  f.write(join(f.config, 'automations/tasks/task.json'), JSON.stringify(task));
  f.write(join(f.state, 'acp/installations/receipt.json'), JSON.stringify({ files: [join(f.state, 'acp/agents/demo/bin/run')], configuration: repository }));
  f.write(join(f.state, 'inbox/selected/executions.json'), JSON.stringify([{ cwd: repository, markdown: repository }]));
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  const copied = join(f.destination, 'data/repos/global');
  assert.equal(git(copied, 'status', '--porcelain'), '');
  assert.equal(readFileSync(join(copied, 'example.json'), 'utf8'), content);
  for (const [target] of untouched) assert.equal(readFileSync(join(f.destination, target), 'utf8'), content, target);
  const migrated = JSON.parse(readFileSync(join(f.destination, 'automations/tasks/task.json')));
  assert.deepEqual(migrated, { ...task, repository_path: copied, workspace_path: copied, launch: { executable: copied + '/run', args: [copied], environment: { AGENT_HOME: copied } } });
  assert.deepEqual(JSON.parse(readFileSync(join(f.destination, 'cache/acp/installations/receipt.json'))), { files: [join(f.destination, 'cache/acp/agents/demo/bin/run')], configuration: repository });
  assert.deepEqual(JSON.parse(readFileSync(join(f.destination, 'state/inbox/executions.json'))), [{ cwd: copied, markdown: repository }]);
});

test('ACP JSONC relocation changes only launch paths and preserves unknown fields and formatting', t => {
  const f = fixture(t);
  const source = join(f.state, 'acp/agents/demo/bin/run');
  const value = JSON.stringify(source);
  const jsonc = `/* ${source} */\r\n{"agent_servers":{"demo":{"command":${value},"args":[${value},"--flag"],"env":{"ROOT":${value}},"unknown":{"command":${value},"enabled":true/* adjacent comment */,"limit":1// line comment\r\n},},},"description":${value},}\r\n`;
  f.write(join(f.config, 'acp/settings.json'), jsonc);
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  const replacement = JSON.stringify(join(f.destination, 'cache/acp/agents/demo/bin/run'));
  const expected = jsonc.replace(`"command":${value}`, `"command":${replacement}`).replace(`"args":[${value}`, `"args":[${replacement}`).replace(`"ROOT":${value}`, `"ROOT":${replacement}`);
  assert.equal(readFileSync(join(f.destination, 'aow-acp.json'), 'utf8'), expected);
});

test('relative symlinks retain their targets across layout changes and external Notes directories', t => {
  const f = fixture(t);
  const link = join(f.state, 'repos/global/adapter');
  mkdirSync(dirname(link), { recursive: true });
  symlinkSync('../../acp/agents/demo/bin/run', link);
  rmSync(join(f.state, 'data'), { recursive: true });
  f.write(join(f.home, 'notes-external/note.md'), 'external notes');
  symlinkSync('notes-external', join(f.home, 'aow'));
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(realpathSync(join(f.destination, 'data/repos/global/adapter')), join(f.destination, 'cache/acp/agents/demo/bin/run'));
  assert.equal(readFileSync(join(f.destination, 'data/notes/note.md'), 'utf8'), 'external notes');
  assert.equal(readlinkSync(join(f.home, 'aow')), 'notes-external');
  assert.equal(readlinkSync(link), '../../acp/agents/demo/bin/run');
  const repeated = f.run();
  assert.equal(repeated.status, 0, repeated.stderr);
  assert.match(repeated.stdout, /0 to copy/);
});

for (const name of ['用户 有空格', "用户 'quote", '用户 "quote\\path']) {
  test(`environment readers preserve the migrated home literally: ${name}`, t => {
    const f = fixture(t, name);
    f.write(join(f.home, '.config/aow/server.env'), `AOW_SERVER_STATE_DIR='${f.state}'\nAOW_STATE_DIR='${f.state}'\n`);
    const result = f.run();
    assert.equal(result.status, 0, result.stderr);
    const env = { ...process.env, HOME: f.home };
    for (const key of Object.keys(env)) if (key.startsWith('AOW_')) delete env[key];
    const command = spawnSync('bash', ['-c', 'source scripts/server-state-dir.sh; server_state_dir'], { env, encoding: 'utf8' });
    assert.equal(command.status, 0, command.stderr);
    assert.equal(command.stdout, f.destination);
    const plist = join(f.home, 'server.plist');
    const generated = spawnSync(process.execPath, ['scripts/launchd-service.mjs', 'server', join(f.home, '.local/lib/aow'), plist], { env, encoding: 'utf8' });
    assert.equal(generated.status, 0, generated.stderr);
    const read = spawnSync('python3', ['-c', 'import json, plistlib, sys; print(json.dumps(plistlib.load(open(sys.argv[1], "rb"))["EnvironmentVariables"]))', plist], { encoding: 'utf8' });
    assert.equal(read.status, 0, read.stderr);
    const variables = JSON.parse(read.stdout);
    assert.equal(variables.AOW_STATE_DIR, f.destination);
    assert.equal(variables.AOW_SERVER_STATE_DIR, f.destination);
  });
}
