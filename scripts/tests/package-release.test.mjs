import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, closeSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, readlinkSync, readdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const machine = process.arch === 'arm64' ? 'aarch64' : 'x86_64';
const target = `${machine}-unknown-linux-musl`;

function write(path, content, executable = false) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content, { mode: executable ? 0o755 : 0o644 });
}

function fixture(t) {
  const temp = realpathSync(mkdtempSync(join(tmpdir(), 'aow-package-')));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const repo = join(temp, 'repo with spaces');
  const home = join(temp, 'home');
  const runtime = join(home, '.local/lib/aow');
  const tools = join(temp, 'tools');
  const artifacts = join(repo, 'target', target, 'release');
  const rustLibdir = join(temp, 'rustlib', target, 'lib');
  write(join(rustLibdir, 'libstd-fixture.rlib'), 'fixture');
  mkdirSync(tools);
  write(join(tools, 'uname'), `#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo ${machine};; esac\n`, true);
  mkdirSync(join(repo, 'vt-worker'), { recursive: true });
  for (const path of [
    'justfile', 'scripts/package-release.sh', 'scripts/install-release.sh',
    'scripts/start-server.sh', 'scripts/start-terminald.sh', 'scripts/replace-symlink.mjs', 'scripts/server-state-dir.sh',
    'scripts/start-launchd.sh', 'scripts/launchd-service.mjs', 'scripts/activate-terminald.mjs', 'scripts/service-health.mjs',
    'packaging/bin/aow', 'packaging/bin/aow-server', 'packaging/bin/aow-terminald',
    'packaging/systemd/aow-server.service', 'packaging/systemd/aow-terminald.service',
  ]) {
    mkdirSync(dirname(join(repo, path)), { recursive: true });
    copyFileSync(join(root, path), join(repo, path));
    if (path.endsWith('.sh')) chmodSync(join(repo, path), 0o755);
  }
  for (const name of ['aow-cli', 'aow-server', 'aow-terminald', 'aow-automation-runner']) {
    write(join(artifacts, name), '#!/bin/sh\n[ "${FAIL_BINARY:-}" != 1 ]\n', true);
  }
  write(join(repo, 'frontend/dist/index.html'), '<html>release frontend</html>');
  write(join(repo, 'frontend/dist/assets/app.js'), 'console.log("release")');
  for (const name of ['cargo', 'npm']) {
    write(join(tools, name), `#!/bin/sh
printf '%s %s\\n' '${name}' "$*" >> "$BUILD_LOG"
[ "\${FAIL_BUILD:-}" != 1 ]
`, true);
  }
  write(join(tools, 'rustc'), '#!/bin/sh\nprintf "%s\\n" "$MOCK_RUST_LIBDIR"\n', true);
  write(join(tools, 'musl-gcc'), '#!/bin/sh\nexit 0\n', true);
  write(join(tools, 'readelf'), '#!/bin/sh\nfor file in "$@"; do :; done\nif [ "${USE_REAL_READELF:-}" = "${file##*/}" ]; then exec /usr/bin/readelf "$@"; fi\nprintf "%s\\n" "$*" >> "$ELF_LOG"\n', true);
  write(join(tools, 'systemctl'), '#!/bin/sh\nprintf "%s\\n" "$*" >> "$SERVICE_LOG"\n[ "${ALLOW_SERVICES:-}" = 1 ] || exit 99\ncase "$*" in *is-active*) exit 1;; esac\n', true);
  symlinkSync(process.execPath, join(tools, 'node'));
  const buildLog = join(temp, 'build.log');
  const serviceLog = join(temp, 'service.log');
  const env = {
    ...process.env,
    HOME: home, PATH: `${tools}:${process.env.PATH}`,
    AOW_RUNTIME_ROOT: runtime, AOW_USER_BIN_DIR: join(home, '.local/bin'),
    AOW_STATE_DIR: join(home, '.local/state/aow'), AOW_SERVER_STATE_DIR: '',
    XDG_CONFIG_HOME: join(home, '.config'), XDG_STATE_HOME: join(home, '.local/state'),
    BUILD_LOG: buildLog, SERVICE_LOG: serviceLog, ALLOW_SERVICES: '1',
    MOCK_ARCH: machine, MOCK_RUST_LIBDIR: rustLibdir, ELF_LOG: join(temp, 'elf.log'),
    [`CC_${target.replaceAll('-', '_')}`]: 'musl-gcc',
  };
  write(join(env.AOW_STATE_DIR, 'pin.md5'), 'e10adc3949ba59abbe56e057f20f883e\n');
  for (const args of [['init'], ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '--allow-empty', '-m', 'fixture']]) {
    assert.equal(spawnSync('git', args, { cwd: repo, env }).status, 0);
  }
  function run(command, args, extra = {}) {
    const stdout = join(temp, 'stdout');
    const stderr = join(temp, 'stderr');
    const fds = [openSync('/dev/null', 'r'), openSync(stdout, 'w'), openSync(stderr, 'w')];
    try {
      const result = spawnSync(command, args, { cwd: repo, env: { ...env, ...extra }, stdio: fds, timeout: 30000, detached: true });
      assert.ifError(result.error);
      return { ...result, stdout: readFileSync(stdout, 'utf8'), stderr: readFileSync(stderr, 'utf8') };
    } finally {
      fds.forEach(closeSync);
    }
  }
  const pack = (args = [], extra = {}) => run('bash', ['scripts/package-release.sh', ...args], extra);
  const log = path => existsSync(path) ? readFileSync(path, 'utf8') : '';
  function assertClean() {
    assert.equal(existsSync(join(repo, 'target/.package.lock')), false);
  }
  return { repo, temp, home, runtime, tools, artifacts, rustLibdir, buildLog, serviceLog, run, pack, log, assertClean };
}

test('full native packaging builds four binaries and both frontends without installing services', t => {
  const f = fixture(t);
  const packed = f.run('just', ['package', '--version', '1.2.0']);
  assert.equal(packed.status, 0, packed.stderr);
  assert.equal(packed.stdout.trim(), join(f.repo, 'target/packages/aow-1.2.0.tar.gz'));
  const builds = f.log(f.buildLog);
  assert.equal((builds.match(/^cargo /gm) ?? []).length, 4);
  for (const line of builds.split('\n').filter(line => line.startsWith('cargo '))) {
    assert.ok(line.includes(`--target ${target}`), line);
    assert.ok(line.includes(`--target-dir ${join(f.repo, 'target')}`), line);
  }
  assert.equal((builds.match(/^npm run build$/gm) ?? []).length, 2);
  assert.equal(f.log(join(f.temp, 'elf.log')).trim().split('\n').length, 4);
  assert.equal(existsSync(f.runtime), false);
  assert.equal(f.log(f.serviceLog), '');
  f.assertClean();
});

test('default version and skip-build create a complete package without invoking build tools', t => {
  const f = fixture(t);
  const result = f.pack(['--skip-build'], { FAIL_BUILD: '1' });
  assert.equal(result.status, 0, result.stderr);
  const archive = result.stdout.trim();
  const checksum = createHash('sha256').update(readFileSync(archive)).digest('hex');
  assert.equal(readFileSync(`${archive}.sha256`, 'utf8'), `${checksum}  ${archive.split('/').at(-1)}\n`);
  assert.match(archive, /aow-\d{8}T\d{6}Z-[a-f0-9]{12}(?:-\d+)?\.tar\.gz$/);
  assert.equal(f.log(f.buildLog), '');
  const extracted = join(f.temp, 'extracted');
  mkdirSync(extracted);
  assert.equal(spawnSync('tar', ['-xzf', archive, '-C', extracted]).status, 0);
  const manifest = JSON.parse(readFileSync(join(extracted, 'manifest.json')));
  assert.equal(manifest.reused_build_artifacts, true);
  assert.deepEqual(manifest.platform, { os: 'linux', arch: machine, target, libc: 'musl', linkage: 'static' });
  assert.equal(archive.endsWith(`aow-${manifest.release_id}.tar.gz`), true);
  for (const path of ['bin/aow-server', 'bin/aow-terminald', 'bin/aow-automation-runner',
    'bin/aow-cli', 'frontend/dist/index.html', 'scripts/start-server.sh', 'scripts/start-terminald.sh',
    'scripts/replace-symlink.mjs', 'scripts/server-state-dir.sh', 'packaging/bin/aow', 'packaging/bin/aow-server', 'packaging/bin/aow-terminald',
    'packaging/systemd/aow-server.service', 'packaging/systemd/aow-terminald.service']) {
    assert.equal(existsSync(join(extracted, path)), true, path);
  }
  assert.equal(existsSync(f.runtime), false);
});

test('just install deploys the latest local package or an explicit path without downloads', t => {
  const f = fixture(t);
  write(join(f.tools, 'curl'), '#!/bin/sh\necho unexpected download >&2\nexit 99\n', true);
  const missing = f.run('just', ['install']);
  assert.notEqual(missing.status, 0);
  assert.match(missing.stderr, /Build it with just package first/);
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  assert.equal(f.pack(['--version', 'local-1']).status, 0);
  const installed = f.run('just', ['install']);
  assert.equal(installed.status, 0, installed.stdout + installed.stderr);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/local-1');
  assert.match(installed.stdout, /Skipped terminald/);
  assert.equal(existsSync(join(f.runtime, 'active/terminald')), false);
  const output = join(f.temp, 'custom packages');
  const packed = f.pack(['--version', 'local-2', '--output-dir', output]);
  assert.equal(packed.status, 0, packed.stderr);
  const explicit = f.run('just', ['install', packed.stdout.trim()]);
  assert.equal(explicit.status, 0, explicit.stdout + explicit.stderr);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/local-2');
  assert.equal(existsSync(join(f.runtime, '.install.lock')), false);
});

test('an existing checksum is never overwritten or selected by latest', t => {
  const f = fixture(t);
  assert.equal(f.pack(['--version', 'good']).status, 0);
  const checksum = join(f.repo, 'target/packages/aow-conflict.tar.gz.sha256');
  write(checksum, 'keep existing checksum');
  const result = f.pack(['--version', 'conflict']);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /checksum already exists/);
  assert.equal(readFileSync(checksum, 'utf8'), 'keep existing checksum');
  assert.equal(existsSync(join(f.repo, 'target/packages/aow-conflict.tar.gz')), false);
  assert.equal(readlinkSync(join(f.repo, 'target/packages/latest')), 'aow-good.tar.gz');
  f.assertClean();
});

test('any dynamic glibc binary is rejected even in the musl directory with skip-build', { skip: process.platform !== 'linux' }, t => {
  const f = fixture(t);
  assert.equal(f.pack(['--version', '1.0.0']).status, 0);
  for (const name of ['aow-server', 'aow-terminald', 'aow-automation-runner', 'aow-cli']) {
    const path = join(f.artifacts, name);
    const original = readFileSync(path);
    copyFileSync('/bin/true', path);
    const result = f.pack(['--version', 'bad', '--skip-build'], { USE_REAL_READELF: name });
    assert.notEqual(result.status, 0, name);
    assert.ok(result.stderr.includes(`${name} is not a static musl release`), result.stderr);
    assert.equal(readlinkSync(join(f.repo, 'target/packages/latest')), 'aow-1.0.0.tar.gz');
    assert.equal(existsSync(join(f.repo, 'target/packages/aow-bad.tar.gz')), false);
    writeFileSync(path, original);
  }
  f.assertClean();
});

test('missing musl tools fail before building and native artifacts never substitute for musl ones', t => {
  const f = fixture(t);
  rmSync(join(f.rustLibdir, 'libstd-fixture.rlib'));
  const missingTarget = f.pack();
  assert.notEqual(missingTarget.status, 0);
  assert.ok(missingTarget.stderr.includes(`rustup target add ${target}`), missingTarget.stderr);
  write(join(f.rustLibdir, 'libstd-fixture.rlib'), 'fixture');
  const missingCompiler = f.pack([], { [`CC_${target.replaceAll('-', '_')}`]: '/nonexistent/musl-gcc' });
  assert.notEqual(missingCompiler.status, 0);
  assert.match(missingCompiler.stderr, /musl C compiler is missing/);
  assert.equal(f.log(f.buildLog), '');
  for (const name of readdirSync(f.artifacts)) {
    write(join(f.repo, 'target/release', name), readFileSync(join(f.artifacts, name)), true);
  }
  rmSync(f.artifacts, { recursive: true });
  const missingArtifacts = f.pack(['--skip-build']);
  assert.notEqual(missingArtifacts.status, 0);
  assert.match(missingArtifacts.stderr, /build the .*linux-musl release first/);
  assert.equal(existsSync(join(f.repo, 'target/packages/latest')), false);
});

for (const failure of ['missing binary', 'asset symlink', 'archive failure', 'build failure', 'bad binary']) {
  test(`${failure} does not advance the local package pointer`, t => {
    const f = fixture(t);
    assert.equal(f.pack(['--version', '1.0.0']).status, 0);
    if (failure === 'missing binary') rmSync(join(f.artifacts, 'aow-cli'));
    if (failure === 'asset symlink') symlinkSync('/tmp', join(f.repo, 'frontend/dist/outside'));
    if (failure === 'archive failure') write(join(f.tools, 'tar'), '#!/bin/sh\nexit 1\n', true);
    const extra = failure === 'build failure' ? { FAIL_BUILD: '1' } : failure === 'bad binary' ? { FAIL_BINARY: '1' } : {};
    const result = f.pack(['--version', '1.2.0'], extra);
    assert.notEqual(result.status, 0);
    assert.equal(readlinkSync(join(f.repo, 'target/packages/latest')), 'aow-1.0.0.tar.gz');
    assert.equal(existsSync(join(f.repo, 'target/packages/aow-1.2.0.tar.gz')), false);
    assert.equal(f.log(f.serviceLog), '');
    assert.equal(existsSync(join(f.repo, 'target/.package.lock')), false);
    assert.equal(readdirSync(join(f.repo, 'target/packages')).some(name => name.startsWith('.package')), false);
    f.assertClean();
  });
}
