import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir, userInfo } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { publishTestRelease } from './github-fixture.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
const files = ['scripts/install-release.sh', 'scripts/package-release.sh',
  'scripts/start-server.sh', 'scripts/start-terminald.sh', 'scripts/start-launchd.sh', 'scripts/launchd-service.mjs',
  'scripts/activate-terminald.mjs', 'scripts/service-health.mjs',
  'scripts/launchd-mode.mjs', 'scripts/start-launchdaemon.mjs', 'scripts/register-launchdaemon.py',
  'scripts/replace-symlink.mjs', 'scripts/server-state-dir.sh', 'packaging/bin/aow',
  'packaging/bin/aow-server', 'packaging/bin/aow-terminald',
  'packaging/systemd/aow-server.service', 'packaging/systemd/aow-terminald.service'];
const binaries = ['aow-server', 'aow-terminald', 'aow-cli', 'aow-automation-runner'];
function write(path, data, executable = false) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, data, { mode: executable ? 0o755 : 0o600 });
}
function fixture(t, arch = 'aarch64') {
  const temp = realpathSync(mkdtempSync(join(tmpdir(), 'aow-macos-release-')));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const repo = join(temp, 'source with spaces');
  const home = join(temp, "home & 'quotes'");
  const runtime = join(home, '.local/lib/aow');
  const tools = join(temp, 'tools');
  const output = join(temp, 'downloads');
  const serviceLog = join(temp, 'services.log');
  const serviceState = join(temp, 'service-state');
  const daemonDirectory = join(temp, 'LaunchDaemons');
  const artifacts = join(repo, 'target', `${arch}-apple-darwin`, 'release');
  for (const path of files) {
    let content = readFileSync(join(root, path), 'utf8');
    if (path === 'scripts/launchd-mode.mjs') {
      // Isolated stand-ins for root-owned plists; never touch system jobs.
      content = content.replace("'/Library/LaunchDaemons'", JSON.stringify(daemonDirectory))
        .replace('const daemonOwner = 0;', `const daemonOwner = ${process.getuid()};`);
    }
    write(join(repo, path), content, !path.endsWith('.mjs'));
  }
  for (const binary of binaries) write(join(artifacts, binary), '#!/bin/sh\nexit 0\n', true);
  write(join(repo, 'frontend/dist/index.html'), '<html>macOS release</html>');
  write(join(home, '.local/state/aow/pin.md5'), 'e10adc3949ba59abbe56e057f20f883e\n');
  // Lifecycle tests mock the health transport; service-health.test.mjs exercises
  // real HTTP and Unix sockets, including mismatched PIDs and startup delays.
  write(join(tools, 'node'), `#!/bin/sh
case "$1" in
  */service-health.mjs)
    component=\${3##*.}
    if [ "\${FAIL_HEALTH:-}" = "$component" ]; then
      case "$(readlink "$AOW_RUNTIME_ROOT/active/$component")" in *2.0.0) exit 1;; esac
    fi
    exit 0;;
esac
exec ${quote(process.execPath)} "$@"
`, true);
  write(join(tools, 'uname'), `#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo ${arch === 'aarch64' ? 'arm64' : arch};; esac\n`, true);
  write(join(tools, 'lipo'), `#!/bin/sh\necho "\${MOCK_BINARY_ARCH:-${arch === 'aarch64' ? 'arm64' : arch}}"\n`, true);
  write(join(tools, 'otool'), `#!/bin/sh
if [ "$1" = -l ]; then
  minimum=\${MOCK_MINOS:-11.0}
  case "$2" in *automation-runner) minimum=\${MOCK_RUNNER_MINOS:-$minimum};; esac
  printf 'Load command 0\\n cmd LC_BUILD_VERSION\\n platform 1\\n minos %s\\n sdk 26.0\\n' "$minimum"
else
  printf '%s:\\n\\t%s (compatibility version 1.0.0, current version 1.0.0)\\n' "$2" "\${MOCK_LIBRARY:-/usr/lib/libSystem.B.dylib}"
fi
`, true);
  write(join(tools, 'sw_vers'), '#!/bin/sh\necho "${MOCK_MACOS_VERSION:-26.7}"\n', true);
  write(join(tools, 'plutil'), `#!/bin/sh
mode=$1
for last in "$@"; do :; done
exec python3 -c 'import json,plistlib,sys; value=plistlib.load(open(sys.argv[1], "rb")); print(json.dumps(value)) if sys.argv[2]=="-convert" else None' "$last" "$mode"
`, true);
  write(join(tools, 'ps'), `#!/bin/sh\necho "\${MOCK_PROCESS_UID:-${process.getuid()}}"\n`, true);
  write(join(tools, 'kill'), '#!/bin/sh\nprintf "signal %s\\n" "$*" >> "$SERVICE_LOG"\n', true);
  write(join(tools, 'launchctl'), `#!/bin/sh
printf '%s\\n' "$*" >> "$SERVICE_LOG"
name=\${2##*/}
case "$1" in
  print)
    case "$2" in
      gui/[0-9]*)
        case "$2" in */org.aow.*) ;; *)
          if [ "\${GUI_QUERY_ERROR:-}" = 1 ]; then echo 'Operation not permitted' >&2; exit 1; fi
          if [ "\${FAIL_GUI:-}" = 1 ]; then echo 'Could not find domain for user' >&2; exit 112; fi
          exit 0;; esac;;
    esac
    if [ -f "$SERVICE_STATE/$name.unloading" ]; then
      remaining=$(cat "$SERVICE_STATE/$name.unloading")
      if [ "$remaining" -gt 0 ]; then
        echo "$((remaining - 1))" > "$SERVICE_STATE/$name.unloading"
        printf 'state = terminating\\n'
        exit 0
      fi
      rm -f "$SERVICE_STATE/$name" "$SERVICE_STATE/$name.unloading"
    fi
    if [ ! -f "$SERVICE_STATE/$name" ]; then echo 'Could not find service' >&2; exit 113; fi
    printf 'state = running\\npid = 987654\\n';;
  enable) exit 0;;
  bootout)
    if [ -f "$SERVICE_STATE/$name" ] && [ "\${BOOTOUT_POLLS:-0}" -gt 0 ]; then
      [ -f "$SERVICE_STATE/$name.unloading" ] || echo "$BOOTOUT_POLLS" > "$SERVICE_STATE/$name.unloading"
    else
      rm -f "$SERVICE_STATE/$name"
    fi;;
  bootstrap)
    name=\${3##*/}; name=\${name%.plist}; component=\${name##*.}
    if [ -f "$SERVICE_STATE/$name" ]; then
      printf 'Bootstrap failed: 5: Input/output error (previous job still registered)\\n' >&2
      exit 5
    fi
    if [ "\${FAIL_LAUNCH:-}" = "$component" ]; then
      case "$(readlink "$AOW_RUNTIME_ROOT/active/$component")" in *2.0.0) exit 5;; esac
    fi
    mkdir -p "$SERVICE_STATE"; touch "$SERVICE_STATE/$name";;
  *) exit 99;;
esac
`, true);
  write(join(tools, 'curl'), `#!/bin/sh
while [ "$#" -gt 0 ]; do
  case "$1" in --output) output=$2; shift 2;; *) url=$1; shift;; esac
done
printf '%s\\n' "$url" >> "$DOWNLOAD_LOG"
case "$url" in
  */releases/download/*) path=releases/\${url#*/releases/download/};;
  */releases/latest/download/aow-install.sh) path=aow-install.sh;;
  *) exit 22;;
esac
if [ ! -f "$DOWNLOAD_ROOT/$path" ]; then printf 404; exit 22; fi
cp "$DOWNLOAD_ROOT/$path" "$output"
`, true);
  const env = { ...process.env, HOME: home, PATH: `${tools}:${process.env.PATH}`, SHELL: '/bin/zsh',
    AOW_RUNTIME_ROOT: runtime, AOW_USER_BIN_DIR: join(home, '.local/bin'),
    AOW_STATE_DIR: '', AOW_SERVER_STATE_DIR: '', AOW_RELEASE_REPOSITORY: '',
    AOW_SERVER_HOST: '', AOW_SERVER_PORT: '', AOW_BASE_PATH: '', AOW_TERMINALD_SOCKET: '',
    XDG_STATE_HOME: '', XDG_RUNTIME_DIR: '', XDG_CONFIG_HOME: join(home, '.config'),
    SERVICE_LOG: serviceLog, SERVICE_STATE: serviceState, DOWNLOAD_ROOT: output,
    DOWNLOAD_LOG: join(temp, 'downloads.log') };
  for (const args of [['init'], ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '--allow-empty', '-m', 'fixture']]) {
    assert.equal(spawnSync('git', args, { cwd: repo, env }).status, 0);
  }
  const run = (command, args, extra = {}, reply) => {
    const options = { cwd: repo, env: { ...env, ...extra }, encoding: 'utf8', timeout: 30000, detached: true };
    const result = reply === undefined ? spawnSync(command, args, options)
      : spawnSync('python3', [join(root, 'scripts/tests/pty-command.py'), [command, ...args].map(quote).join(' ')], { ...options, input: reply });
    assert.ifError(result.error);
    return result;
  };
  const pack = (version, extra = {}) => run('bash', ['scripts/package-release.sh', '--version', version, '--skip-build'], extra);
  const publish = (version, archive = join(repo, `target/packages/aow-${version}.tar.gz`)) => {
    publishTestRelease(output, archive, version, `macos-${arch}`);
    return { status: 0, stdout: '', stderr: '' };
  };
  const install = (args = [], extra = {}, reply) => run('bash', [join(output, 'aow-install.sh'), ...args], extra, reply);
  const log = path => existsSync(path) ? readFileSync(path, 'utf8') : '';
  const plist = component => join(home, `Library/LaunchAgents/org.aow.${component}.plist`);
  const registerDaemons = () => {
    const request = JSON.parse(readFileSync(join(runtime, 'pending-launchdaemon.json'), 'utf8'));
    for (const config of Object.values(request.components)) {
      const path = join(daemonDirectory, `${config.Label}.plist`);
      mkdirSync(daemonDirectory, { recursive: true });
      const rendered = spawnSync('python3', ['-c', 'import json,plistlib,sys; sys.stdout.buffer.write(plistlib.dumps(json.load(sys.stdin)))'],
        { input: JSON.stringify(config), encoding: 'utf8' });
      succeeds(rendered);
      write(path, rendered.stdout);
      write(join(serviceState, config.Label), '');
    }
  };
  return { temp, repo, home, runtime, tools, output, artifacts, env, serviceLog, run, pack, publish, install, log, plist, daemonDirectory, registerDaemons };
}
function succeeds(result) { assert.equal(result.status, 0, result.stdout + result.stderr); }

test('launchd removal has a bounded wait and never loads over a job that is still registered', t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0')); succeeds(f.install());
  const previousPlist = f.log(f.plist('server'));
  succeeds(f.pack('2.0.0')); succeeds(f.publish('2.0.0'));
  // Skip the delay, retaining the same bounded poll count.
  write(join(f.tools, 'sleep'), '#!/bin/sh\nexit 0\n', true);
  writeFileSync(f.serviceLog, '');
  const failed = f.install([], { BOOTOUT_POLLS: '10000' });
  assert.notEqual(failed.status, 0);
  assert.match(failed.stdout + failed.stderr, /timed out waiting for launchd to remove/);
  assert.match(failed.stdout + failed.stderr, /failed to restore previous server LaunchAgent/);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap/);
  assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/1.0.0'));
  assert.equal(f.log(f.plist('server')), previousPlist);
  assert.equal(existsSync(join(f.runtime, '.install.lock')), false);
});

for (const source of ['local', 'github']) {
  test(`${source} install waits for asynchronous launchd removal on upgrade and rollback`, t => {
    const f = fixture(t);
    const install = (extra = {}) => source === 'local'
      ? f.run('/bin/bash', ['scripts/install-release.sh', '--package', 'target/packages/latest'], extra, 'y\n')
      : f.install([], extra, 'y\n');
    succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0')); succeeds(install());
    const previousServer = f.log(f.plist('server'));
    const previousTerminald = f.log(f.plist('terminald'));
    const previousNode = readlinkSync(join(f.runtime, 'node'));
    succeeds(f.pack('2.0.0')); succeeds(f.publish('2.0.0'));
    const failed = install({ BOOTOUT_POLLS: '3', FAIL_HEALTH: 'server' });
    assert.notEqual(failed.status, 0);
    assert.match(failed.stdout + failed.stderr, /activation failed/);
    assert.doesNotMatch(failed.stdout + failed.stderr, /Bootstrap failed/);
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/1.0.0'));
    assert.equal(f.log(f.plist('server')), previousServer);
    succeeds(f.run('launchctl', ['print', `gui/${process.getuid()}/org.aow.server`]));
    const terminaldFailed = install({ BOOTOUT_POLLS: '3', FAIL_HEALTH: 'terminald' });
    assert.notEqual(terminaldFailed.status, 0);
    assert.match(terminaldFailed.stdout + terminaldFailed.stderr, /activation failed/);
    assert.doesNotMatch(terminaldFailed.stdout + terminaldFailed.stderr, /Bootstrap failed/);
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/2.0.0'));
    assert.equal(realpathSync(join(f.runtime, 'active/terminald')), join(f.runtime, 'releases/1.0.0'));
    assert.equal(f.log(f.plist('terminald')), previousTerminald);
    assert.equal(readlinkSync(join(f.runtime, 'node')), previousNode);
    succeeds(f.run('launchctl', ['print', `gui/${process.getuid()}/org.aow.terminald`]));
    succeeds(install({ BOOTOUT_POLLS: '3' }));
    for (const component of ['server', 'terminald']) {
      assert.equal(realpathSync(join(f.runtime, `active/${component}`)), join(f.runtime, 'releases/2.0.0'));
    }
    assert.equal(existsSync(join(f.runtime, '.install.lock')), false);
  });
}

for (const arch of ['aarch64', 'x86_64']) {
  test(`macOS ${arch}: local packages share launchd activation, rollback and terminald confirmation`, t => {
    const f = fixture(t, arch);
    const install = (path = 'target/packages/latest', extra = {}, reply) =>
      f.run('/bin/bash', ['scripts/install-release.sh', '--package', path], extra, reply);
    succeeds(f.pack('local-1'));
    succeeds(install(undefined, {}, 'y\n'));
    const serverPlist = f.log(f.plist('server'));
    const terminaldPlist = f.log(f.plist('terminald'));
    const updateSettings = f.log(join(f.runtime, 'update.json'));
    succeeds(f.pack('2.0.0'));
    const failed = install(undefined, { FAIL_LAUNCH: 'server' });
    assert.notEqual(failed.status, 0);
    assert.match(failed.stdout + failed.stderr, /activation failed/);
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/local-1'));
    assert.equal(f.log(f.plist('server')), serverPlist);
    assert.equal(f.log(join(f.runtime, 'update.json')), updateSettings);
    succeeds(install(undefined, {}, 'n\n'));
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/2.0.0'));
    assert.equal(realpathSync(join(f.runtime, 'active/terminald')), join(f.runtime, 'releases/local-1'));
    assert.equal(f.log(f.plist('terminald')), terminaldPlist);
    succeeds(install('target/packages/aow-local-1.tar.gz'));
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/local-1'));
    assert.equal(f.log(f.env.DOWNLOAD_LOG), '');
    assert.equal(existsSync(join(f.runtime, '.install.lock')), false);
  });

  test(`macOS ${arch}: GitHub downloads and verifies the matching release asset`, t => {
    const f = fixture(t, arch);
    succeeds(f.pack('1.0.0'));
    const asset = `aow-macos-${arch}.tar.gz`;
    succeeds(f.publish('1.0.0'));
    const base = 'https://github.com/yorkart/aow/releases';
    succeeds(f.install(['--version', '1.0.0']));
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/1.0.0'));
    assert.equal(f.log(f.env.DOWNLOAD_LOG), `${base}/download/1.0.0/SHA256SUMS\n${base}/download/1.0.0/${asset}\n`);
    assert.equal(JSON.parse(f.log(join(f.runtime, 'update.json'))).repository, 'yorkart/aow');
  });

  test(`macOS ${arch}: package, publish, piped install, update and version rollback`, t => {
    const f = fixture(t, arch);
    succeeds(f.pack('1.0.0'));
    succeeds(f.publish('1.0.0'));
    const installer = join(f.output, 'aow-install.sh');
    succeeds(f.run('bash', ['-c', `cat ${quote(installer)} | bash`], {}, 'y\n'));
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/1.0.0'));
    assert.equal(realpathSync(join(f.runtime, 'active/terminald')), join(f.runtime, 'releases/1.0.0'));
    const terminaldPlist = f.log(f.plist('terminald'));
    for (const component of ['server', 'terminald']) {
      const plist = f.log(f.plist(component));
      assert.match(plist, /AOW_LOG_MODE<\/key><string>unified/);
      assert.doesNotMatch(plist, /AOW_SERVICE_LOG|Library\/Logs\/AOW/);
    }
    assert.equal(existsSync(join(f.home, 'Library/Logs/AOW')), false);
    const nodeTarget = readlinkSync(join(f.runtime, 'node'));
    write(join(f.home, '.config/aow/server.env'), 'AOW_SERVER_PORT=8283\n');
    succeeds(f.pack('2.0.0'));
    succeeds(f.publish('2.0.0'));
    const manager = join(f.home, '.local/bin/aow');
    succeeds(f.run(manager, ['update'], {}, 'n\n'));
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/2.0.0'));
    assert.equal(realpathSync(join(f.runtime, 'active/terminald')), join(f.runtime, 'releases/1.0.0'));
    assert.equal(f.log(f.plist('terminald')), terminaldPlist);
    assert.equal(readlinkSync(join(f.runtime, 'node')), nodeTarget);
    assert.match(f.log(f.plist('server')), /AOW_SERVER_PORT<\/key><string>8283/);
    assert.match(f.log(f.plist('server')), /home &amp; &apos;quotes&apos;/);
    succeeds(f.install(['--version', '1.0.0']));
    assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/1.0.0'));
    assert.equal(JSON.parse(f.log(join(f.runtime, 'update.json'))).repository, 'yorkart/aow');
    assert.match(f.log(f.env.DOWNLOAD_LOG), new RegExp(`/aow-macos-${arch}\\.tar\\.gz`));
    assert.doesNotMatch(f.log(f.serviceLog), /systemctl|kickstart/);
    assert.equal(f.log(join(f.home, '.local/state/aow/pin.md5')), 'e10adc3949ba59abbe56e057f20f883e\n');
  });
}

for (const component of ['server', 'terminald']) {
  test(`launchd ${component} activation failure restores its previous release and plist`, t => {
    const f = fixture(t);
    succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0')); succeeds(f.install([], {}, 'y\n'));
    const before = f.log(f.plist(component));
    const nodeTarget = readlinkSync(join(f.runtime, 'node'));
    succeeds(f.pack('2.0.0')); succeeds(f.publish('2.0.0'));
    const failed = f.install([], { FAIL_LAUNCH: component }, 'y\n');
    assert.notEqual(failed.status, 0);
    assert.match(failed.stdout + failed.stderr, /activation failed/);
    assert.equal(realpathSync(join(f.runtime, `active/${component}`)), join(f.runtime, 'releases/1.0.0'));
    assert.equal(f.log(f.plist(component)), before);
    assert.equal(readlinkSync(join(f.runtime, 'node')), nodeTarget);
    assert.equal(existsSync(join(f.runtime, '.install.lock')), false);
  });
}

test('macOS package rejects wrong architecture and non-system dynamic libraries before publication', t => {
  const f = fixture(t);
  for (const extra of [{ MOCK_BINARY_ARCH: 'x86_64' }, { MOCK_LIBRARY: '/opt/homebrew/lib/libdependency.dylib' }]) {
    assert.notEqual(f.pack('bad', extra).status, 0);
    assert.equal(existsSync(join(f.repo, 'target/packages/latest')), false);
  }
});

for (const component of ['server', 'terminald']) {
  test(`launchd ${component} running without a healthy endpoint restores the previous release`, t => {
    const f = fixture(t);
    succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0')); succeeds(f.install([], {}, 'y\n'));
    const before = f.log(f.plist(component));
    const nodeTarget = readlinkSync(join(f.runtime, 'node'));
    succeeds(f.pack('2.0.0')); succeeds(f.publish('2.0.0'));
    const result = f.install([], { FAIL_HEALTH: component }, 'y\n');
    assert.notEqual(result.status, 0);
    assert.match(result.stdout + result.stderr, /activation failed/);
    assert.equal(realpathSync(join(f.runtime, `active/${component}`)), join(f.runtime, 'releases/1.0.0'));
    assert.equal(f.log(f.plist(component)), before);
    assert.equal(readlinkSync(join(f.runtime, 'node')), nodeTarget);
  });
}

test('macOS package records the highest binary deployment target and rejects older hosts before activation', t => {
  const f = fixture(t);
  succeeds(f.pack('newer', { MOCK_MINOS: '11.0', MOCK_RUNNER_MINOS: '14.2' }));
  const archive = join(f.repo, 'target/packages/aow-newer.tar.gz');
  const manifest = JSON.parse(f.run('tar', ['-xOf', archive, './manifest.json']).stdout);
  assert.equal(manifest.platform.minimum_os_version, '14.2');
  succeeds(f.publish('newer'));
  const result = f.install([], { MOCK_MACOS_VERSION: '14.1.9' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /requires macOS 14.2/);
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout/);
});

test('macOS first install sets a hidden PIN with the system Bash and then starts both services', t => {
  const f = fixture(t);
  rmSync(join(f.home, '.local/state/aow/pin.md5'));
  succeeds(f.pack('first')); succeeds(f.publish('first'));
  const result = f.run('/bin/bash', ['-c', `cat ${quote(join(f.output, 'aow-install.sh'))} | /bin/bash`], {}, '123456\n123456\ny\n');
  succeeds(result);
  assert.doesNotMatch(result.stdout + result.stderr, /123456/);
  assert.equal(f.log(join(f.home, '.local/state/aow/pin.md5')), 'e10adc3949ba59abbe56e057f20f883e\n');
  assert.equal(existsSync(f.plist('server')), true);
  assert.equal(existsSync(f.plist('terminald')), true);
});

test('a missing macOS release asset fails without changing services', t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0'));
  rmSync(join(f.output, 'releases/1.0.0/aow-macos-aarch64.tar.gz'));
  assert.notEqual(f.install().status, 0);
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout/);
});

test('macOS installer validates the package platform before running binaries or switching releases', t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0'));
  write(join(f.tools, 'uname'), '#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo x86_64;; esac\n', true);
  publishTestRelease(f.output, join(f.repo, 'target/packages/aow-1.0.0.tar.gz'), '1.0.0', 'macos-x86_64');
  const result = f.install(['--version', '1.0.0']);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /package platform does not match/);
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout/);
});

test('macOS installer does not treat GUI inspection errors as a headless account', t => {
  const f = fixture(t);
  const alias = join(f.temp, 'mode-link.mjs');
  symlinkSync(join(f.repo, 'scripts/launchd-mode.mjs'), alias);
  const mode = f.run(process.execPath, [alias, 'resolve', f.runtime]);
  succeeds(mode);
  assert.equal(mode.stdout, 'launchagent\n');
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0'));
  const result = f.install([], { GUI_QUERY_ERROR: '1' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /graphical user session/);
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout/);
});

test('saved LaunchAgent mode persists without a GUI and rejects account/configuration mismatches', t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0')); succeeds(f.install());
  const path = join(f.runtime, 'update.json');
  const settings = JSON.parse(f.log(path));
  assert.deepEqual(settings.service, { mode: 'launchagent', user: userInfo().username, uid: process.getuid() });
  writeFileSync(f.serviceLog, '');
  const missing = f.install([], { FAIL_GUI: '1' });
  assert.notEqual(missing.status, 0);
  assert.match(missing.stderr, /will not silently switch modes/);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout/);
  settings.service.user = 'another-account';
  writeFileSync(path, JSON.stringify(settings));
  const mismatch = f.install();
  assert.notEqual(mismatch.status, 0);
  assert.match(mismatch.stderr, /does not match the installation account/);
});

test('legacy LaunchAgent installations retain their mode before update.json has a service record', t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0')); succeeds(f.install());
  const path = join(f.runtime, 'update.json');
  const settings = JSON.parse(f.log(path));
  delete settings.service;
  settings.custom_setting = 'preserved';
  writeFileSync(path, JSON.stringify(settings));
  assert.notEqual(f.install([], { FAIL_GUI: '1' }).status, 0);
  succeeds(f.install());
  assert.equal(JSON.parse(f.log(path)).custom_setting, 'preserved');
  assert.equal(JSON.parse(f.log(path)).service.mode, 'launchagent');
});

test('headless install stages registration, persists mode, updates without sudo and rolls back failures', {
  skip: process.getuid() === 0,
}, t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0'));
  const pending = f.install([], { FAIL_GUI: '1' }, 'y\n');
  assert.equal(pending.status, 78, pending.stdout + pending.stderr);
  assert.match(pending.stdout + pending.stderr, /registration is pending/);
  const settings = JSON.parse(f.log(join(f.runtime, 'update.json')));
  assert.equal(settings.service.mode, 'launchdaemon');
  const request = JSON.parse(f.log(join(f.runtime, 'pending-launchdaemon.json')));
  assert.deepEqual(Object.keys(request.components).sort(), ['server', 'terminald']);
  for (const config of Object.values(request.components)) {
    assert.equal(config.UserName, userInfo().username);
    assert.equal(config.KeepAlive, true);
    assert.equal(config.EnvironmentVariables.HOME, f.home);
    assert.equal(config.StandardOutPath, '/dev/null');
  }
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout|signal/);
  f.registerDaemons();
  succeeds(f.install([], { FAIL_GUI: '1' }, 'y\n'));
  assert.equal(existsSync(join(f.runtime, 'pending-launchdaemon.json')), false);
  const managedNode = realpathSync(join(f.runtime, 'node'));
  succeeds(f.install([], { PATH: `${f.runtime}:${f.env.PATH}` }, 'y\n'));
  assert.equal(realpathSync(join(f.runtime, 'node')), managedNode);
  assert.notEqual(readlinkSync(join(f.runtime, 'node')), join(f.runtime, 'node'));
  succeeds(f.pack('2.0.0')); succeeds(f.publish('2.0.0'));
  // Even with a GUI now present, aow update must retain the saved daemon mode.
  const manager = join(f.home, '.local/bin/aow');
  succeeds(f.run(manager, ['update'], {}, 'n\n'));
  assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/2.0.0'));
  assert.equal(realpathSync(join(f.runtime, 'active/terminald')), join(f.runtime, 'releases/1.0.0'));
  const failed = f.install([], { FAIL_HEALTH: 'terminald' }, 'y\n');
  assert.notEqual(failed.status, 0);
  assert.equal(realpathSync(join(f.runtime, 'active/terminald')), join(f.runtime, 'releases/1.0.0'));
  assert.match(f.log(f.serviceLog), /signal -TERM 987654/);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout|kickstart/);
  assert.equal(existsSync(f.plist('server')), false);
  assert.equal(existsSync(join(f.runtime, '.launchdaemon-activation.lock')), false);
});

test('daemon config changes require registration without replacing active releases; foreign PID is never signalled', {
  skip: process.getuid() === 0,
}, t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0'));
  assert.equal(f.install([], { FAIL_GUI: '1' }, 'y\n').status, 78);
  f.registerDaemons();
  succeeds(f.install([], {}, 'y\n'));
  succeeds(f.pack('2.0.0')); succeeds(f.publish('2.0.0'));
  write(join(f.home, '.config/aow/server.env'), 'AOW_SERVER_PORT=8283\n');
  writeFileSync(f.serviceLog, '');
  assert.equal(f.install([], {}, 'n\n').status, 78);
  assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/1.0.0'));
  assert.doesNotMatch(f.log(f.serviceLog), /signal|bootstrap|bootout/);
  f.registerDaemons();
  const refused = f.install([], { MOCK_PROCESS_UID: String(process.getuid() + 1) }, 'n\n');
  assert.notEqual(refused.status, 0);
  assert.match(refused.stdout + refused.stderr, /different account/);
  assert.doesNotMatch(f.log(f.serviceLog), /signal/);
  assert.equal(realpathSync(join(f.runtime, 'active/server')), join(f.runtime, 'releases/1.0.0'));
});

test('pending headless installation retains its mode before registration and leaves unconfirmed terminald untouched', {
  skip: process.getuid() === 0,
}, t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0'));
  assert.equal(f.install([], { FAIL_GUI: '1' }).status, 78);
  // A graphical login after preparation must not change the saved mode.
  assert.equal(f.install().status, 78);
  const request = JSON.parse(f.log(join(f.runtime, 'pending-launchdaemon.json')));
  assert.deepEqual(Object.keys(request.components), ['server']);
  assert.equal(existsSync(join(f.runtime, 'active/terminald')), false);
  assert.equal(existsSync(join(f.runtime, 'node')), false);
  assert.equal(existsSync(f.plist('server')), false);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout|signal/);
  f.registerDaemons();
  succeeds(f.install());
  rmSync(join(f.home, '.local/state/aow/pin.md5'));
  writeFileSync(f.serviceLog, '');
  const missingPin = f.run('/bin/sh', ['scripts/start-server.sh', '1.0.0']);
  assert.notEqual(missingPin.status, 0);
  assert.match(missingPin.stderr, /交互终端/);
  assert.doesNotMatch(f.log(f.serviceLog), /signal|bootstrap|bootout/);
  // Conflicting registrations cannot be resolved by silently changing mode.
  write(f.plist('server'), '<plist/>');
  const conflict = f.install();
  assert.notEqual(conflict.status, 0);
  assert.match(conflict.stderr, /Both LaunchAgent and LaunchDaemon/);
});

test('daemon installation refuses a system configuration belonging to another account', {
  skip: process.getuid() === 0,
}, t => {
  const f = fixture(t);
  succeeds(f.pack('1.0.0')); succeeds(f.publish('1.0.0'));
  assert.equal(f.install([], { FAIL_GUI: '1' }).status, 78);
  const pending = join(f.runtime, 'pending-launchdaemon.json');
  const request = JSON.parse(f.log(pending));
  request.components.server.UserName = 'another-service-account';
  writeFileSync(pending, JSON.stringify(request));
  f.registerDaemons();
  const conflict = f.install();
  assert.notEqual(conflict.status, 0);
  assert.match(conflict.stderr, /belong to another account/);
  assert.doesNotMatch(f.log(f.serviceLog), /bootstrap|bootout|signal/);
});

test('native macOS Mach-O binaries pass the real packaging checks', { skip: process.platform !== 'darwin' }, t => {
  const arch = process.arch === 'arm64' ? 'aarch64' : 'x86_64';
  const f = fixture(t, arch);
  const source = join(f.temp, 'main.c');
  write(source, 'int main(void) { return 0; }\n');
  for (const binary of binaries) succeeds(f.run('/usr/bin/clang', [source, '-o', join(f.artifacts, binary)]));
  write(join(f.tools, 'lipo'), '#!/bin/sh\nexec /usr/bin/lipo "$@"\n', true);
  write(join(f.tools, 'otool'), '#!/bin/sh\nexec /usr/bin/otool "$@"\n', true);
  succeeds(f.pack('native'));
  const plist = join(f.temp, 'native.plist');
  succeeds(f.run(process.execPath, ['scripts/launchd-service.mjs', 'server', f.runtime, plist]));
  succeeds(f.run('/usr/bin/plutil', ['-lint', plist]));
});

test('confirmed terminald activation finishes after its waiting parent receives SIGHUP', t => {
  const temp = mkdtempSync(join(tmpdir(), 'aow-detached-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const marker = join(temp, 'completed');
  const script = join(temp, 'activation.sh');
  write(script, `#!/bin/sh\nkill -HUP "$PPID"\nprintf done > ${quote(marker)}\n`);
  const result = spawnSync(process.execPath, [join(root, 'scripts/activate-terminald.mjs'), script], { timeout: 5000 });
  assert.ifError(result.error);
  assert.equal(result.signal, 'SIGHUP');
  assert.equal(readFileSync(marker, 'utf8'), 'done');
});

test('real AoW macOS release installs with isolated mocked services', {
  skip: !process.env.AOW_NATIVE_TEST_PACKAGE,
}, t => {
  const f = fixture(t, process.arch === 'arm64' ? 'aarch64' : 'x86_64');
  const manifest = JSON.parse(f.run('tar', ['-xOf', process.env.AOW_NATIVE_TEST_PACKAGE, './manifest.json']).stdout);
  succeeds(f.publish(manifest.release_id, process.env.AOW_NATIVE_TEST_PACKAGE));
  succeeds(f.install());
  assert.equal(existsSync(f.plist('server')), true);
  assert.equal(existsSync(join(f.runtime, 'active/terminald')), false);
  assert.equal(existsSync(join(f.home, '.local/state/aow/config-repo')), true);
});
