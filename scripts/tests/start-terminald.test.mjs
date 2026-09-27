import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { closeSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, readlinkSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

function fixture(t, active = true) {
  const temp = mkdtempSync(join(tmpdir(), 'aow-start-terminald-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const repo = join(temp, 'repo with spaces');
  const runtime = join(temp, 'runtime');
  const tools = join(temp, 'tools');
  const unit = join(temp, 'config/systemd/user/aow-terminald.service');
  const serviceLog = join(temp, 'service.log');
  const write = (path, content) => {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content, { mode: 0o755 });
  };

  mkdirSync(join(repo, 'scripts'), { recursive: true });
  copyFileSync(join(root, 'scripts/start-terminald.sh'), join(repo, 'scripts/start-terminald.sh'));
  write(join(repo, 'packaging/systemd/aow-terminald.service'), 'new unit\n');
  // All service commands are intercepted; no real daemon is started or contacted.
  write(join(tools, 'systemctl'), `#!/bin/sh
printf '%s\\n' "$*" >> "$SERVICE_LOG"
case "$*" in
  '--user show-environment'|'--user daemon-reload'|'--user enable --now aow-terminald.service'|'--user restart aow-terminald.service') exit 0 ;;
  '--user is-active --quiet aow-terminald.service') exit "$MOCK_ACTIVE_STATUS" ;;
  *) exit 99 ;;
esac
`);
  write(join(tools, 'node'), '#!/bin/sh\nprintf "v24.0.0\\n"\n');
  write(join(tools, 'uname'), '#!/bin/sh\nprintf "Linux\\n"\n');
  for (const name of ['bin/aow-terminald', 'releases/old/bin/aow-terminald', 'releases/new/bin/aow-terminald']) {
    write(join(runtime, name), '#!/bin/sh\nexit 99\n');
  }
  mkdirSync(join(runtime, 'active'));
  symlinkSync('../releases/old', join(runtime, 'active/terminald'));
  symlinkSync('releases/new', join(runtime, 'latest'));
  symlinkSync('/old/node', join(runtime, 'node'));
  write(join(runtime, '.aow-terminald-node-target'), '/old/node\n');
  write(unit, 'old unit\n');

  const env = {
    ...process.env,
    HOME: join(temp, 'home'),
    XDG_CONFIG_HOME: join(temp, 'config'),
    AOW_RUNTIME_ROOT: runtime,
    PATH: `${tools}:${process.env.PATH}`,
    SERVICE_LOG: serviceLog,
    MOCK_ACTIVE_STATUS: active ? '0' : '3',
  };
  const run = input => {
    const stdin = join(temp, 'stdin');
    const stdout = join(temp, 'stdout');
    const stderr = join(temp, 'stderr');
    writeFileSync(stdin, input);
    const fds = [openSync(stdin, 'r'), openSync(stdout, 'w'), openSync(stderr, 'w')];
    try {
      const result = spawnSync('sh', [join(repo, 'scripts/start-terminald.sh')], {
        cwd: repo, env, stdio: fds, timeout: 30000,
      });
      return { ...result, stdout: readFileSync(stdout, 'utf8'), stderr: readFileSync(stderr, 'utf8') };
    } finally {
      fds.forEach(closeSync);
    }
  };
  const assertUnchanged = () => {
    assert.equal(existsSync(serviceLog), false, 'even service queries must wait for confirmation');
    assert.equal(readlinkSync(join(runtime, 'active/terminald')), '../releases/old');
    assert.equal(readlinkSync(join(runtime, 'node')), '/old/node');
    assert.equal(readFileSync(join(runtime, '.aow-terminald-node-target'), 'utf8'), '/old/node\n');
    assert.equal(readFileSync(unit, 'utf8'), 'old unit\n');
  };
  return { run, assertUnchanged, runtime, tools, unit, serviceLog };
}

for (const [name, input] of [
  ['n', 'n\n'], ['empty line', '\n'], ['EOF', ''], ['y without Enter', 'y'],
  ['uppercase Y', 'Y\n'], ['yes', 'yes\n'], ['leading space', ' y\n'],
  ['trailing space', 'y \n'], ['escaped y', '\\y\n'],
]) {
  test(`start-terminald cancels without side effects for ${name}`, t => {
    const f = fixture(t);
    const result = f.run(input);
    assert.ifError(result.error);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /重启会终止它管理的所有终端会话/);
    assert.match(result.stderr, /请输入 y 并回车/);
    assert.match(result.stderr, /已取消/);
    f.assertUnchanged();
  });
}

for (const active of [true, false]) {
  test(`y and Enter allows ${active ? 'restart' : 'initial start'} using mocked services`, t => {
    const f = fixture(t, active);
    const result = f.run('y\n');
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(readFileSync(f.serviceLog, 'utf8').trim().split('\n'), [
      '--user show-environment',
      '--user is-active --quiet aow-terminald.service',
      '--user daemon-reload',
      '--user enable --now aow-terminald.service',
      ...(active ? ['--user restart aow-terminald.service'] : []),
    ]);
    assert.equal(readlinkSync(join(f.runtime, 'active/terminald')), '../releases/new');
    assert.equal(readlinkSync(join(f.runtime, 'node')), join(f.tools, 'node'));
    assert.equal(readFileSync(f.unit, 'utf8'), 'new unit\n');
    assert.match(result.stdout, /aow-terminald is active on release new/);
  });
}
