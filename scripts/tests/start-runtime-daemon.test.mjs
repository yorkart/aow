import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { closeSync, copyFileSync, mkdirSync, mkdtempSync, openSync, readFileSync, readlinkSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

function fixture(t, active = true) {
  const temp = mkdtempSync(join(tmpdir(), 'aow-start-runtime-daemon-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const repo = join(temp, 'repo with spaces');
  const runtime = join(temp, 'runtime');
  const tools = join(temp, 'tools');
  const unit = join(temp, 'config/systemd/user/aow-runtime-daemon.service');
  const serviceLog = join(temp, 'service.log');
  const write = (path, content) => {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content, { mode: 0o755 });
  };

  mkdirSync(join(repo, 'scripts'), { recursive: true });
  for (const file of ['start-runtime-daemon.sh', 'activate-runtime.mjs']) {
    copyFileSync(join(root, 'scripts', file), join(repo, 'scripts', file));
  }
  write(join(repo, 'packaging/systemd/aow-runtime-daemon.service'), 'new unit\n');
  // All service commands are intercepted; no real daemon is started or contacted.
  write(join(tools, 'systemctl'), `#!/bin/sh
printf '%s\\n' "$*" >> "$SERVICE_LOG"
case "$*" in
  '--user show-environment'|'--user daemon-reload'|'--user enable --now aow-runtime-daemon.service'|'--user restart aow-runtime-daemon.service') exit 0 ;;
  '--user is-active --quiet aow-runtime-daemon.service') exit "$MOCK_ACTIVE_STATUS" ;;
  *) exit 99 ;;
esac
`);
  symlinkSync(process.execPath, join(tools, 'node'));
  write(join(tools, 'uname'), '#!/bin/sh\nprintf "Linux\\n"\n');
  for (const name of ['bin/aow-runtime-daemon', 'releases/old/bin/aow-runtime-daemon', 'releases/new/bin/aow-runtime-daemon']) {
    write(join(runtime, name), '#!/bin/sh\nexit 99\n');
  }
  mkdirSync(join(runtime, 'active'));
  symlinkSync('../releases/old', join(runtime, 'active/runtime-daemon'));
  symlinkSync('releases/new', join(runtime, 'latest'));
  symlinkSync('/old/node', join(runtime, 'node'));
  write(join(runtime, '.aow-runtime-daemon-node-target'), '/old/node\n');
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
      const result = spawnSync('sh', [join(repo, 'scripts/start-runtime-daemon.sh')], {
        cwd: repo, env, stdio: fds, timeout: 30000,
      });
      return { ...result, stdout: readFileSync(stdout, 'utf8'), stderr: readFileSync(stderr, 'utf8') };
    } finally {
      fds.forEach(closeSync);
    }
  };
  return { run, runtime, tools, unit, serviceLog };
}

for (const active of [true, false]) {
  test(`runtime activation permits ${active ? 'restart' : 'initial start'} using mocked services`, t => {
    const f = fixture(t, active);
    const result = f.run('y\n');
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(readFileSync(f.serviceLog, 'utf8').trim().split('\n'), [
      '--user show-environment',
      '--user is-active --quiet aow-runtime-daemon.service',
      '--user daemon-reload',
      '--user enable --now aow-runtime-daemon.service',
      ...(active ? ['--user restart aow-runtime-daemon.service'] : []),
    ]);
    assert.equal(readlinkSync(join(f.runtime, 'active/runtime-daemon')), '../releases/new');
    assert.equal(readlinkSync(join(f.runtime, 'node')), realpathSync(process.execPath));
    assert.equal(readFileSync(f.unit, 'utf8'), 'new unit\n');
    assert.match(result.stdout, /aow-runtime-daemon is active on release new/);
  });
}
