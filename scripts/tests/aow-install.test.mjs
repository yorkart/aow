import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { closeSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, readlinkSync, readdirSync, rmSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { publishTestRelease } from './github-fixture.mjs';
import { platforms, prepareGitHubRelease } from '../prepare-github-release.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const installer = join(root, 'scripts/install-release.sh');
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
const digest = pin => `${createHash('md5').update(pin).digest('hex')}\n`;
const platform = 'linux-x86_64';

function write(path, content, executable = false) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content, { mode: executable ? 0o755 : 0o644 });
}

function fixture(t, { symlinkHome = false, withPin = true } = {}) {
  const temp = mkdtempSync(join(tmpdir(), 'aow-download-install-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const home = join(temp, 'home with spaces');
  if (symlinkHome) {
    const realHome = join(temp, 'real home');
    mkdirSync(realHome);
    symlinkSync(realHome, home);
  }
  const runtime = join(home, '.local/lib/aow');
  const userBin = join(home, '.local/bin');
  const downloads = join(temp, 'downloads');
  const tools = join(temp, 'tools');
  const serviceLog = join(temp, 'services.log');
  const downloadLog = join(temp, 'downloads.log');
  const initializeLog = join(temp, 'initialize.log');
  mkdirSync(downloads);
  copyFileSync(installer, join(downloads, 'aow-install.sh'));
  mkdirSync(tools);
  write(join(tools, 'uname'), '#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo x86_64;; esac\n', true);
  symlinkSync(process.execPath, join(tools, 'node'));
  write(join(tools, 'curl'), `#!/bin/sh
output=
url=
head=0
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) output=$2; shift 2 ;;
        --head) head=1; shift ;;
        *) url=$1; shift ;;
    esac
done
printf '%s\\n' "$url" >> "$DOWNLOAD_LOG"
[ "\${FAIL_DOWNLOAD:-}" != 1 ] || exit 22
if [ "$head" = 1 ]; then
    if [ -n "\${MOCK_GITHUB_LATEST_URL:-}" ]; then printf '%s' "$MOCK_GITHUB_LATEST_URL"
    else printf '%s/tag/%s' "\${url%/latest}" "$(cat "$DOWNLOAD_ROOT/latest")"; fi
    exit 0
fi
case "$url" in
    https://github.com/*/releases/download/*)
        path=\${url#*/releases/download/}
        cp "$DOWNLOAD_ROOT/releases/$path" "$output"
        exit $? ;;
esac
cp "$DOWNLOAD_ROOT/\${url##*/}" "$output"
`, true);
  write(join(tools, 'systemctl'), `#!/bin/sh
printf '%s\\n' "$*" >> "$SERVICE_LOG"
case "$*" in
    '--user show-environment') exit "\${FAIL_MANAGER:-0}" ;;
    '--user is-active --quiet aow-server.service'|'--user is-active --quiet aow-terminald.service') exit "\${SERVICE_INACTIVE:-0}" ;;
    '--user daemon-reload') exit 0 ;;
    '--user enable --now aow-server.service'|'--user enable --now aow-terminald.service') exit 0 ;;
    '--user restart aow-server.service') exit "\${FAIL_SERVER_RESTART:-0}" ;;
    '--user restart aow-terminald.service') exit "\${FAIL_TERMINALD_RESTART:-0}" ;;
    *) exit 99 ;;
esac
`, true);
  const env = {
    ...process.env,
    HOME: home,
    PATH: `${tools}:${process.env.PATH}`,
    XDG_CONFIG_HOME: join(home, '.config'),
    XDG_STATE_HOME: join(home, '.local/state'),
    AOW_RUNTIME_ROOT: runtime,
    AOW_USER_BIN_DIR: userBin,
    AOW_RELEASE_REPOSITORY: '',
    AOW_STATE_DIR: join(temp, 'state'),
    AOW_SERVER_STATE_DIR: '',
    DOWNLOAD_ROOT: downloads,
    DOWNLOAD_LOG: downloadLog,
    SERVICE_LOG: serviceLog,
    INITIALIZE_LOG: initializeLog,
  };
  const pinFile = join(env.AOW_STATE_DIR, 'pin.md5');
  if (withPin) write(pinFile, digest('654321'));
  function pack(version = '1.2.3', customize = () => {}) {
    const bundle = join(temp, `bundle-${version}`);
    for (const name of ['aow-cli', 'aow-terminald', 'aow-automation-runner']) {
      write(join(bundle, 'bin', name), '#!/bin/sh\n[ "${FAIL_BINARY:-}" != 1 ]\n', true);
    }
    write(join(bundle, 'bin/aow-server'), '#!/bin/sh\nprintf "%s\\n" "$@" >> "$INITIALIZE_LOG"\n[ "${FAIL_INITIALIZE:-}" != 1 ]\n', true);
    write(join(bundle, 'frontend/dist/index.html'), '<html>AOW</html>');
    write(join(bundle, 'manifest.json'), JSON.stringify({ release_id: version, platform: { os: 'linux', arch: 'x86_64' } }));
    for (const path of [
      'scripts/start-server.sh', 'scripts/start-terminald.sh', 'scripts/replace-symlink.mjs', 'scripts/server-state-dir.sh',
      'packaging/bin/aow', 'packaging/bin/aow-server', 'packaging/bin/aow-terminald',
      'packaging/systemd/aow-server.service', 'packaging/systemd/aow-terminald.service',
    ]) {
      mkdirSync(dirname(join(bundle, path)), { recursive: true });
      copyFileSync(join(root, path), join(bundle, path));
    }
    customize(bundle);
    const archive = join(downloads, `aow-${version}.tar.gz`);
    const packed = spawnSync('tar', ['-czf', archive, '-C', bundle, '.'], { encoding: 'utf8' });
    assert.equal(packed.status, 0, packed.stderr);
    return publishTestRelease(downloads, archive, version, platform);
  }
  function runCommand(command, extra = {}, reply) {
    const stdin = join(temp, 'stdin');
    const stdout = join(temp, 'stdout');
    const stderr = join(temp, 'stderr');
    writeFileSync(stdin, reply ?? '');
    const fds = [openSync(stdin, 'r'), openSync(stdout, 'w'), openSync(stderr, 'w')];
    const options = { env: { ...env, ...extra }, timeout: 30000, detached: true, stdio: fds };
    try {
      // A portable PTY keeps PIN/confirmation input separate from the pipe.
      const result = reply !== undefined
        ? spawnSync('python3', [join(root, 'scripts/tests/pty-command.py'), command], options)
        : spawnSync('bash', ['-c', command], options);
      return { ...result, stdout: readFileSync(stdout, 'utf8'), stderr: readFileSync(stderr, 'utf8') };
    } finally {
      fds.forEach(closeSync);
    }
  }
  const run = (args = [], extra = {}, reply) => runCommand(
    `cat ${quote(join(downloads, 'aow-install.sh'))} | bash -s -- ${args.map(quote).join(' ')}`, extra, reply);
  const manager = (args = ['update'], extra = {}, reply, binDir = userBin) => runCommand(
    `${quote(join(binDir, 'aow'))} ${args.map(quote).join(' ')}`,
    { AOW_RELEASE_REPOSITORY: '', AOW_USER_BIN_DIR: '', ...extra }, reply);
  const log = path => existsSync(path) ? readFileSync(path, 'utf8') : '';
  function assertClean() {
    assert.equal(existsSync(join(runtime, '.install.lock')), false);
    if (existsSync(runtime)) assert.equal(readdirSync(runtime).some(name => name.startsWith('.download.')), false);
    if (existsSync(runtime)) assert.equal(readdirSync(runtime).some(name => name.startsWith('.update.')), false);
  }
  return { temp, home, runtime, userBin, downloads, env, pinFile, serviceLog, downloadLog, initializeLog, pack, run, runCommand, manager, log, assertClean };
}

async function githubRelease(f, version) {
  const revision = 'a'.repeat(40);
  const inputDir = join(f.temp, `github-input-${version}`);
  const outputDir = join(f.temp, `generated-${version}`);
  mkdirSync(inputDir);
  for (const platform of platforms) {
    const [os, arch] = platform.split('-');
    const archive = f.pack(version, bundle => write(join(bundle, 'manifest.json'), JSON.stringify({
      release_id: version, git_revision: revision, git_dirty: false, reused_build_artifacts: false,
      platform: { os, arch, target: `${arch}-${os === 'linux' ? 'unknown-linux-musl' : 'apple-darwin'}`,
        ...(os === 'linux' ? { libc: 'musl', linkage: 'static' } : {}) },
    })));
    copyFileSync(archive, join(inputDir, `aow-${platform}.tar.gz`));
  }
  await prepareGitHubRelease({ version, revision, repository: 'yorkart/aow', inputDir, outputDir });
  cpSync(outputDir, join(f.downloads, 'releases', version), { recursive: true });
  copyFileSync(join(outputDir, 'aow-install.sh'), join(f.downloads, 'aow-install.sh'));
  return join(f.downloads, 'releases', version);
}

const githubBase = 'https://github.com/yorkart/aow/releases/latest/download';
const githubDownloads = 'https://github.com/yorkart/aow/releases/download';

function localPackage(f, version = 'local-1', customize) {
  f.pack(version, customize);
  const archive = join(f.downloads, `aow-${version}.tar.gz`);
  const hash = createHash('sha256').update(readFileSync(archive)).digest('hex');
  writeFileSync(`${archive}.sha256`, `${hash}  ${basename(archive)}\n`);
  return archive;
}

test('local install uses the same PIN, upgrade and terminald lifecycle and preserves GitHub update settings', t => {
  const f = fixture(t, { withPin: false, symlinkHome: true });
  const archive = localPackage(f);
  const latest = join(f.downloads, 'local latest');
  symlinkSync(basename(archive), latest);
  const local = (file, args = [], extra = {}, reply) => f.runCommand(
    `bash ${quote(installer)} --package ${quote(file)} ${args.map(quote).join(' ')}`, { FAIL_DOWNLOAD: '1', ...extra }, reply);
  const missingPin = local(latest);
  assert.notEqual(missingPin.status, 0);
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  const customBin = join(f.home, 'custom bin');
  const result = local(latest, ['--repo', 'example/fork'], { AOW_USER_BIN_DIR: customBin }, '012345\n012345\ny\n');
  assert.equal(result.status, 0, result.stdout + result.stderr);
  for (const component of ['server', 'terminald']) {
    assert.equal(readlinkSync(join(f.runtime, `active/${component}`)), '../releases/local-1');
  }
  assert.equal(f.log(f.pinFile), digest('012345'));
  const settings = f.log(join(f.runtime, 'update.json'));
  assert.deepEqual(JSON.parse(settings), { repository: 'example/fork', user_bin_dir: customBin });
  const config = join(f.home, '.config/aow/server.env');
  write(config, 'AOW_SERVER_PORT=8283\n');
  const next = localPackage(f, 'local-2');
  rmSync(latest);
  symlinkSync(basename(next), latest);
  const upgraded = local(latest, [], { AOW_USER_BIN_DIR: '' }, 'n\n');
  assert.equal(upgraded.status, 0, upgraded.stdout + upgraded.stderr);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/local-2');
  assert.equal(readlinkSync(join(f.runtime, 'active/terminald')), '../releases/local-1');
  assert.equal(readlinkSync(join(customBin, 'aow-cli')), join(f.runtime, 'releases/local-2/bin/aow-cli'));
  assert.equal(f.log(join(f.runtime, 'update.json')), settings);
  assert.equal(f.log(config), 'AOW_SERVER_PORT=8283\n');
  assert.equal(f.log(f.pinFile), digest('012345'));
  assert.equal(f.log(f.downloadLog), '');
  const reinstalled = local(next, [], { AOW_USER_BIN_DIR: '' });
  assert.equal(reinstalled.status, 0, reinstalled.stdout + reinstalled.stderr);
  assert.match(reinstalled.stdout, /Keeping existing immutable release/);
  f.pack('v3.0.0');
  const updated = f.manager(['update'], {}, undefined, customBin);
  assert.equal(updated.status, 0, updated.stdout + updated.stderr);
  assert.match(f.log(f.downloadLog), /github.com\/example\/fork\/releases/);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/v3.0.0');
  f.assertClean();
});

for (const failure of ['missing package', 'missing checksum', 'corrupt package', 'duplicate checksum', 'wrong checksum name', 'wrong platform', 'unsafe archive']) {
  test(`local ${failure} fails before activation without falling back to GitHub`, t => {
    const f = fixture(t);
    const archive = localPackage(f, 'bad', bundle => {
      if (failure === 'wrong platform') write(join(bundle, 'manifest.json'), JSON.stringify({ release_id: 'bad', platform: { os: 'macos', arch: 'x86_64' } }));
      if (failure === 'unsafe archive') symlinkSync('/tmp', join(bundle, 'outside'));
    });
    const checksum = `${archive}.sha256`;
    if (failure === 'missing package') rmSync(archive);
    if (failure === 'missing checksum') rmSync(checksum);
    if (failure === 'corrupt package') writeFileSync(archive, 'corrupt');
    if (failure === 'duplicate checksum') writeFileSync(checksum, readFileSync(checksum, 'utf8').repeat(2));
    if (failure === 'wrong checksum name') writeFileSync(checksum, readFileSync(checksum, 'utf8').replace(basename(archive), 'other.tar.gz'));
    const result = f.runCommand(`bash ${quote(installer)} --package ${quote(archive)}`, { FAIL_DOWNLOAD: '1' });
    assert.notEqual(result.status, 0, result.stdout + result.stderr);
    assert.equal(existsSync(join(f.runtime, 'latest')), false);
    assert.equal(f.log(f.initializeLog), '');
    assert.doesNotMatch(f.log(f.serviceLog), /restart|enable|daemon-reload/);
    assert.equal(f.log(f.downloadLog), '');
    f.assertClean();
  });
}

test('local package version comes only from the manifest, including with a pinned GitHub installer', t => {
  const f = fixture(t);
  const archive = localPackage(f, 'local-dev');
  f.pack('v9.0.0');
  for (const args of [['--package'], ['--package', ''], ['--package', archive, '--version', 'local-dev']]) {
    const invalid = f.run(args);
    assert.notEqual(invalid.status, 0);
    assert.equal(f.log(f.serviceLog), '');
    assert.equal(f.log(f.downloadLog), '');
  }
  const result = f.run(['--package', archive], { FAIL_DOWNLOAD: '1' });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/local-dev');
  assert.equal(f.log(f.downloadLog), '');
  f.assertClean();
});

test('GitHub piped installer pins its own version even after latest advances, then aow update follows latest', async t => {
  const f = fixture(t);
  const old = await githubRelease(f, 'v1.0.0');
  await githubRelease(f, 'v2.0.0');
  const result = f.runCommand(`cat ${quote(join(old, 'aow-install.sh'))} | bash`, {
    AOW_RELEASE_REPOSITORY: '', MOCK_GITHUB_LATEST_URL: 'https://github.com/yorkart/aow/releases/tag/v2.0.0',
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(f.log(f.downloadLog).trim().split('\n'), [
    `${githubDownloads}/v1.0.0/SHA256SUMS`, `${githubDownloads}/v1.0.0/aow-${platform}.tar.gz`,
  ]);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/v1.0.0');
  assert.equal(JSON.parse(f.log(join(f.runtime, 'update.json'))).repository, 'yorkart/aow');
  writeFileSync(f.downloadLog, '');
  const updated = f.manager();
  assert.equal(updated.status, 0, updated.stdout + updated.stderr);
  assert.deepEqual(f.log(f.downloadLog).trim().split('\n'), [
    `${githubBase}/aow-install.sh`, `${githubDownloads}/v2.0.0/SHA256SUMS`, `${githubDownloads}/v2.0.0/aow-${platform}.tar.gz`,
  ]);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/v2.0.0');
  writeFileSync(f.downloadLog, '');
  const rollback = f.manager(['update', '--version', 'v1.0.0']);
  assert.equal(rollback.status, 0, rollback.stdout + rollback.stderr);
  assert.match(f.log(f.downloadLog), /\/download\/v1\.0\.0\/SHA256SUMS/);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/v1.0.0');
  f.assertClean();
});

test('source installer resolves GitHub latest once and explicit version bypasses resolution', async t => {
  const f = fixture(t);
  await githubRelease(f, 'v1.0.0');
  const latest = f.runCommand(`bash ${quote(installer)}`, {
    MOCK_GITHUB_LATEST_URL: 'https://github.com/yorkart/aow/releases/tag/v1.0.0',
  });
  assert.equal(latest.status, 0, latest.stdout + latest.stderr);
  assert.deepEqual(f.log(f.downloadLog).trim().split('\n'), [
    'https://github.com/yorkart/aow/releases/latest',
    `${githubDownloads}/v1.0.0/SHA256SUMS`, `${githubDownloads}/v1.0.0/aow-${platform}.tar.gz`,
  ]);
  writeFileSync(f.downloadLog, '');
  const explicit = f.run(['--repo', 'yorkart/aow', '--version', 'v1.0.0']);
  assert.equal(explicit.status, 0, explicit.stdout + explicit.stderr);
  assert.doesNotMatch(f.log(f.downloadLog), /latest/);
  f.assertClean();
});

test('Linux ARM64 selects its own GitHub asset', async t => {
  const f = fixture(t);
  await githubRelease(f, 'v1.0.0');
  write(join(f.temp, 'tools/uname'), '#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo aarch64;; esac\n', true);
  const result = f.run(['--repo', 'yorkart/aow', '--version', 'v1.0.0']);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(f.log(f.downloadLog), /\/aow-linux-aarch64\.tar\.gz/);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/v1.0.0');
});

test('switching a GitHub installer to another repository resolves that repository instead of its embedded version', async t => {
  const f = fixture(t);
  const old = await githubRelease(f, 'v1.0.0');
  await githubRelease(f, 'v2.0.0');
  const other = 'https://github.com/another/aow/releases';
  const result = f.runCommand(`bash ${quote(join(old, 'aow-install.sh'))} --repo another/aow`, {
    MOCK_GITHUB_LATEST_URL: `${other}/tag/v2.0.0`,
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(f.log(f.downloadLog).trim().split('\n'), [
    `${other}/latest`, `${other}/download/v2.0.0/SHA256SUMS`, `${other}/download/v2.0.0/aow-${platform}.tar.gz`,
  ]);
  assert.equal(JSON.parse(f.log(join(f.runtime, 'update.json'))).repository, 'another/aow');
});

for (const failure of ['corrupt package', 'missing checksum', 'duplicate checksum', 'missing package']) {
  test(`GitHub ${failure} cannot switch releases or fall back to another download source`, async t => {
    const f = fixture(t);
    f.pack('old');
    assert.equal(f.run().status, 0);
    const output = await githubRelease(f, 'v1.0.0');
    const archive = join(output, `aow-${platform}.tar.gz`);
    const sums = join(output, 'SHA256SUMS');
    if (failure === 'corrupt package') writeFileSync(archive, 'corrupted');
    if (failure === 'missing package') rmSync(archive);
    if (failure === 'missing checksum') writeFileSync(sums, '');
    if (failure === 'duplicate checksum') writeFileSync(sums, readFileSync(sums, 'utf8').repeat(2));
    writeFileSync(f.serviceLog, '');
    writeFileSync(f.downloadLog, '');
    const result = f.manager(['update', '--repo', 'yorkart/aow']);
    assert.notEqual(result.status, 0);
    assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/old');
    assert.equal(JSON.parse(f.log(join(f.runtime, 'update.json'))).repository, 'yorkart/aow');
    assert.doesNotMatch(f.log(f.serviceLog), /restart|enable|daemon-reload/);
    assert.equal(f.log(f.downloadLog).trim().split('\n').length, 3);
    f.assertClean();
  });
}

test('GitHub latest resolution rejects an unexpected redirect before requesting packages', t => {
  const f = fixture(t);
  for (const url of ['https://github.com/login', 'https://github.com/another/aow/releases/tag/v1.0.0',
    'https://github.com/yorkart/aow/releases/tag/../../unsafe']) {
    writeFileSync(f.downloadLog, '');
    const result = f.runCommand(`bash ${quote(installer)}`, { MOCK_GITHUB_LATEST_URL: url });
    assert.notEqual(result.status, 0);
    assert.equal(f.log(f.downloadLog).trim().split('\n').length, 1);
    assert.equal(existsSync(join(f.runtime, 'active/server')), false);
    f.assertClean();
  }
});

test('piped install with an existing PIN uses its pinned GitHub release and skips terminald without a TTY', t => {
  const f = fixture(t);
  f.pack();
  const result = f.run([], { SERVICE_INACTIVE: '1' });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(f.log(f.downloadLog).trim().split('\n'), [
    `${githubDownloads}/1.2.3/SHA256SUMS`,
    `${githubDownloads}/1.2.3/aow-${platform}.tar.gz`,
  ]);
  assert.equal(readlinkSync(join(f.runtime, 'latest')), 'releases/1.2.3');
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/1.2.3');
  assert.equal(readlinkSync(join(f.userBin, 'aow-cli')), join(f.runtime, 'releases/1.2.3/bin/aow-cli'));
  assert.equal(readlinkSync(join(f.userBin, 'aow')), join(f.runtime, 'releases/1.2.3/packaging/bin/aow'));
  assert.deepEqual(readFileSync(join(f.userBin, 'aow')), readFileSync(join(root, 'packaging/bin/aow')));
  assert.equal(existsSync(join(f.runtime, 'bin/aow')), false);
  assert.deepEqual(JSON.parse(f.log(join(f.runtime, 'update.json'))), {
    repository: 'yorkart/aow', user_bin_dir: f.userBin,
  });
  assert.match(f.log(f.serviceLog), /enable --now aow-server/);
  assert.doesNotMatch(f.log(f.serviceLog), /terminald|restart/);
  assert.equal(existsSync(join(f.runtime, 'active/terminald')), false);
  assert.equal(f.log(f.pinFile), digest('654321'));
  assert.doesNotMatch(result.stdout + result.stderr, /请输入.*PIN/);
  assert.match(result.stdout, /Skipped terminald/);
  f.assertClean();
});

test('first piped install requires a confirmed PIN before starting services', t => {
  const f = fixture(t, { withPin: false });
  f.pack();
  const result = f.run([], { SERVICE_INACTIVE: '1' }, '012345\n012345\ny\n');
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(f.log(f.pinFile), digest('012345'));
  assert.equal(statSync(f.pinFile).mode & 0o777, 0o600);
  assert.doesNotMatch(result.stdout + result.stderr, /012345/);
  assert.ok(result.stdout.indexOf('PIN 已保存') < result.stdout.indexOf('aow-server is active'));
  assert.equal((result.stdout.match(/请输入 6 位数字 PIN/g) ?? []).length, 1);
  assert.match(f.log(f.serviceLog), /enable --now aow-server/);
  assert.match(f.log(f.serviceLog), /enable --now aow-terminald/);
  assert.deepEqual(readdirSync(dirname(f.pinFile)), ['pin.md5']);
  f.assertClean();
});

test('PIN setup retries empty, malformed and mismatched input before activation', t => {
  const f = fixture(t, { withPin: false });
  f.pack();
  const result = f.run([], {}, '\n12345\n1234567\n12a456\n123456\n654321\n012345\n012345\nn\n');
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(f.log(f.pinFile), digest('012345'));
  assert.equal((result.stdout.match(/PIN 必须恰好为 6 位数字/g) ?? []).length, 4);
  assert.match(result.stdout, /两次输入的 PIN 不一致/);
  assert.doesNotMatch(result.stdout + result.stderr, /123456|654321|012345/);
  f.assertClean();
});

test('missing PIN without a terminal blocks activation', t => {
  const f = fixture(t, { withPin: false });
  f.pack();
  const result = f.run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /PIN 设置需要交互终端/);
  assert.equal(existsSync(f.pinFile), false);
  assert.equal(existsSync(join(f.runtime, 'active/server')), false);
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  assert.equal(f.log(f.initializeLog), '');
  f.assertClean();
});

test('EOF during PIN confirmation cancels first installation without starting the server', t => {
  const f = fixture(t, { withPin: false });
  f.pack();
  const result = f.run([], {}, '012345\n\x04');
  assert.ifError(result.error);
  assert.notEqual(result.status, 0);
  assert.match(result.stdout, /PIN 输入已取消/);
  assert.equal(existsSync(f.pinFile), false);
  assert.equal(existsSync(join(f.runtime, 'active/server')), false);
  assert.equal(f.log(f.serviceLog), '--user show-environment\n');
  f.assertClean();
});

test('aow pin changes an existing PIN without downloading or touching services', t => {
  const f = fixture(t);
  f.pack();
  assert.equal(f.run().status, 0);
  const services = f.log(f.serviceLog);
  const downloads = f.log(f.downloadLog);
  // A release URL and the update configuration are unnecessary for PIN changes.
  write(join(f.runtime, 'update.json'), 'not JSON');
  const result = f.manager(['pin'], { FAIL_DOWNLOAD: '1', FAIL_MANAGER: '1' }, '012345\n012345\n');
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(f.log(f.pinFile), digest('012345'));
  assert.equal(statSync(f.pinFile).mode & 0o777, 0o600);
  assert.doesNotMatch(result.stdout + result.stderr, /012345/);
  assert.equal(f.log(f.serviceLog), services);
  assert.equal(f.log(f.downloadLog), downloads);
  assert.deepEqual(readdirSync(dirname(f.pinFile)), ['pin.md5']);
});

test('aow pin cancellation, no terminal, help and invalid arguments preserve the PIN', t => {
  const f = fixture(t);
  f.pack();
  assert.equal(f.run().status, 0);
  const services = f.log(f.serviceLog);
  const before = f.log(f.pinFile);
  const cancelled = f.manager(['pin'], {}, '012345\n\x04');
  assert.ifError(cancelled.error);
  assert.notEqual(cancelled.status, 0);
  assert.match(cancelled.stdout, /PIN 输入已取消/);
  const noTerminal = f.manager(['pin']);
  assert.notEqual(noTerminal.status, 0);
  assert.match(noTerminal.stderr, /交互终端/);
  assert.notEqual(f.manager(['pin', '123456']).status, 0);
  const help = f.manager(['pin', '--help']);
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /Usage: aow pin/);
  assert.equal(f.manager(['pin', '--if-missing']).status, 0);
  assert.equal(f.log(f.pinFile), before);
  assert.equal(f.log(f.serviceLog), services);
  assert.deepEqual(readdirSync(dirname(f.pinFile)), ['pin.md5']);
});

for (const source of ['default', 'xdg', 'state environment', 'server.env', 'server environment']) {
  test(`install and aow pin share the ${source} state directory`, t => {
    const f = fixture(t, { withPin: false });
    const state = source === 'default' ? join(f.home, '.local/state/aow')
      : source === 'xdg' ? join(f.temp, 'xdg state/aow') : join(f.temp, 'chosen state');
    const extra = { AOW_STATE_DIR: '', XDG_STATE_HOME: '' };
    if (source === 'xdg') extra.XDG_STATE_HOME = dirname(state);
    if (source === 'state environment') extra.AOW_STATE_DIR = state;
    if (source.startsWith('server')) {
      extra.AOW_STATE_DIR = join(f.temp, 'unused state');
      const configured = source === 'server.env' ? state : join(f.temp, 'unused configured state');
      write(join(f.home, '.config/aow/server.env'), `AOW_SERVER_STATE_DIR="${configured}"\n`);
      if (source === 'server environment') extra.AOW_SERVER_STATE_DIR = state;
    }
    f.pack();
    const installed = f.run([], extra, '012345\n012345\nn\n');
    assert.ifError(installed.error);
    assert.equal(installed.status, 0, installed.stdout + installed.stderr);
    assert.equal(f.log(join(state, 'pin.md5')), digest('012345'));
    assert.ok(f.log(f.initializeLog).includes(`--state-dir\n${state}\n`));
    const changed = f.manager(['pin'], extra, '987654\n987654\n');
    assert.ifError(changed.error);
    assert.equal(changed.status, 0, changed.stdout + changed.stderr);
    assert.equal(f.log(join(state, 'pin.md5')), digest('987654'));
    assert.equal(existsSync(join(f.temp, 'unused state')), false);
    assert.equal(existsSync(join(f.temp, 'unused configured state')), false);
    f.assertClean();
  });
}

test('start-server checks PIN using its packaged command even without the user command link', t => {
  const f = fixture(t);
  f.pack();
  assert.equal(f.run().status, 0);
  rmSync(f.pinFile);
  rmSync(join(f.userBin, 'aow'));
  const before = f.log(f.serviceLog);
  const command = `sh ${quote(join(f.runtime, 'releases/1.2.3/scripts/start-server.sh'))}`;
  const blocked = f.runCommand(command);
  assert.notEqual(blocked.status, 0);
  assert.match(blocked.stderr, /交互终端/);
  assert.equal(f.log(f.serviceLog), `${before}--user show-environment\n`);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/1.2.3');
  const restarted = f.runCommand(command, {}, '012345\n012345\n');
  assert.ifError(restarted.error);
  assert.equal(restarted.status, 0, restarted.stdout + restarted.stderr);
  assert.equal(f.log(f.pinFile), digest('012345'));
  assert.match(f.log(f.serviceLog).slice(before.length), /restart aow-server/);
});

test('explicit version bypasses latest and repository flag overrides environment', t => {
  const f = fixture(t);
  f.pack();
  const result = f.run(['--version', '1.2.3', '--repo', 'override/aow'], { AOW_RELEASE_REPOSITORY: 'environment/aow' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(f.log(f.downloadLog), `https://github.com/override/aow/releases/download/1.2.3/SHA256SUMS\nhttps://github.com/override/aow/releases/download/1.2.3/aow-${platform}.tar.gz\n`);
  f.assertClean();
});

test('installation and update create command links under symlinked HOME', t => {
  const f = fixture(t, { symlinkHome: true });
  f.pack('1.2.3');
  const installed = f.run();
  assert.equal(installed.status, 0, installed.stdout + installed.stderr);
  assert.equal(readlinkSync(join(f.userBin, 'aow')), join(f.runtime, 'releases/1.2.3/packaging/bin/aow'));
  const help = f.manager(['--help']);
  assert.equal(help.status, 0, help.stdout + help.stderr);
  assert.match(help.stdout, /Usage: aow update/);
  f.pack('2.0.0');
  const updated = f.manager();
  assert.equal(updated.status, 0, updated.stdout + updated.stderr);
  assert.equal(readlinkSync(join(f.runtime, 'latest')), 'releases/2.0.0');
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/2.0.0');
  for (const name of ['aow-cli', 'aow-automation-runner']) {
    assert.equal(readlinkSync(join(f.userBin, name)), join(f.runtime, 'releases/2.0.0/bin', name));
  }
  f.assertClean();
});

test('upgrade retains configuration, old releases and unconfirmed terminald state', t => {
  const f = fixture(t);
  f.pack('1.0.0');
  assert.equal(f.run().status, 0);
  mkdirSync(join(f.runtime, 'active'), { recursive: true });
  symlinkSync('../releases/1.0.0', join(f.runtime, 'active/terminald'));
  symlinkSync('/previous/node', join(f.runtime, 'node'));
  write(join(f.runtime, '.aow-terminald-node-target'), '/previous/node\n');
  const config = join(f.home, '.config/aow/server.env');
  const state = join(f.temp, 'existing state path');
  write(config, `AOW_SERVER_PORT=8283\nAOW_SERVER_STATE_DIR="${state}"\n`);
  const pin = join(state, 'pin.md5');
  write(pin, 'existing PIN digest');
  f.pack('1.2.3');
  const result = f.run();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/1.2.3');
  assert.equal(readlinkSync(join(f.runtime, 'active/terminald')), '../releases/1.0.0');
  assert.equal(readlinkSync(join(f.runtime, 'node')), '/previous/node');
  assert.equal(readFileSync(pin, 'utf8'), 'existing PIN digest');
  assert.match(readFileSync(config, 'utf8'), /AOW_SERVER_PORT=8283/);
  assert.ok(f.log(f.initializeLog).includes(`--state-dir\n${state}\n`));
  assert.equal(existsSync(join(f.runtime, 'releases/1.0.0/bin/aow-cli')), true);
  assert.doesNotMatch(f.log(f.serviceLog), /terminald/);
  f.assertClean();
});

for (const reply of ['n\n', '\n', 'Y\n', 'yes\n', ' y\n', 'y \n']) {
  test(`terminal reply ${JSON.stringify(reply)} skips terminald and keeps server active`, t => {
    const f = fixture(t);
    f.pack();
    const result = f.run([], {}, reply);
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    assert.match(f.log(f.serviceLog), /restart aow-server/);
    assert.doesNotMatch(f.log(f.serviceLog), /terminald/);
    assert.match(result.stdout, /Skipped terminald/);
    f.assertClean();
  });
}

test('only y at the controlling terminal starts terminald, after server activation', t => {
  const f = fixture(t);
  f.pack();
  const result = f.run([], {}, 'y\n');
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const services = f.log(f.serviceLog);
  assert.ok(services.indexOf('restart aow-server') < services.indexOf('restart aow-terminald'));
  assert.equal(readlinkSync(join(f.runtime, 'active/terminald')), '../releases/1.2.3');
  assert.equal(existsSync(join(f.runtime, 'node')), true);
  f.assertClean();
});

test('a terminald failure after y is reported as a failure, not a skip', t => {
  const f = fixture(t);
  f.pack();
  const result = f.run([], { FAIL_TERMINALD_RESTART: '1' }, 'y\n');
  assert.notEqual(result.status, 0);
  assert.match(result.stdout, /terminald activation failed/);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/1.2.3');
  f.assertClean();
});

for (const failure of ['FAIL_DOWNLOAD', 'FAIL_BINARY', 'FAIL_INITIALIZE']) {
  test(`${failure} leaves installed pointers and services unchanged`, t => {
    const f = fixture(t);
    f.pack('1.0.0');
    assert.equal(f.run().status, 0);
    const before = f.log(f.serviceLog);
    f.pack();
    const result = f.run([], { [failure]: '1' });
    assert.notEqual(result.status, 0);
    assert.equal(readlinkSync(join(f.runtime, 'latest')), 'releases/1.0.0');
    assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/1.0.0');
    assert.equal(readlinkSync(join(f.userBin, 'aow-cli')), join(f.runtime, 'releases/1.0.0/bin/aow-cli'));
    assert.equal(f.log(f.serviceLog), `${before}--user show-environment\n`);
    f.assertClean();
  });
}

test('reinstall rejects different content for an existing immutable version', t => {
  const f = fixture(t);
  f.pack();
  assert.equal(f.run().status, 0);
  const binary = join(f.runtime, 'releases/1.2.3/bin/aow-cli');
  writeFileSync(binary, '#!/bin/sh\n# existing immutable binary\nexit 0\n');
  const result = f.run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /version already installed with different content/);
  assert.match(readFileSync(binary, 'utf8'), /existing immutable binary/);
  f.assertClean();
});

for (const [name, customize] of [
  ['mismatched manifest', bundle => write(join(bundle, 'manifest.json'), '{"release_id":"other"}')],
  ['missing helper', bundle => rmSync(join(bundle, 'scripts/start-server.sh'))],
  ['missing management command', bundle => rmSync(join(bundle, 'packaging/bin/aow'))],
  ['missing state helper', bundle => rmSync(join(bundle, 'scripts/server-state-dir.sh'))],
  ['symlink', bundle => symlinkSync('/tmp', join(bundle, 'outside'))],
  ['missing binary', bundle => rmSync(join(bundle, 'bin/aow-cli'))],
]) {
  test(`rejects archive with ${name} before installation or service changes`, t => {
    const f = fixture(t);
    f.pack('1.2.3', customize);
    const result = f.run();
    assert.notEqual(result.status, 0);
    assert.equal(existsSync(join(f.runtime, 'latest')), false);
    assert.equal(f.log(f.serviceLog), '--user show-environment\n');
    f.assertClean();
  });
}

test('invalid explicit versions and corrupted archives fail cleanly', t => {
  const f = fixture(t);
  const archive = f.pack();
  for (const invalid of ['../escape', '1.2.3\n1.2.4', '', 'latest']) {
    assert.notEqual(f.run(['--version', invalid]).status, 0);
    f.assertClean();
  }
  writeFileSync(archive, 'not a tar archive');
  assert.notEqual(f.run(['--version', '1.2.3']).status, 0);
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  f.assertClean();
});

test('an unmanaged CLI entry is preserved and prevents activation', t => {
  const f = fixture(t);
  f.pack();
  write(join(f.userBin, 'aow-cli'), 'custom CLI');
  const result = f.run();
  assert.notEqual(result.status, 0);
  assert.equal(readFileSync(join(f.userBin, 'aow-cli'), 'utf8'), 'custom CLI');
  assert.equal(existsSync(join(f.runtime, 'latest')), false);
  assert.equal(f.log(f.serviceLog), '--user show-environment\n');
  f.assertClean();
});


test('aow update uses the saved repository and refreshed installer while preserving state and terminald', t => {
  const f = fixture(t);
  f.pack('1.0.0');
  assert.equal(f.run().status, 0);
  symlinkSync('../releases/1.0.0', join(f.runtime, 'active/terminald'));
  const config = join(f.home, '.config/aow/server.env');
  write(config, 'AOW_SERVER_PORT=8283\n');
  const pin = join(f.env.AOW_STATE_DIR, 'pin.md5');
  write(pin, 'keep-pin');
  f.pack('2.0.0');
  writeFileSync(join(f.downloads, 'aow-install.sh'), readFileSync(join(f.downloads, 'aow-install.sh'), 'utf8')
    .replace('Installed AOW %s in %s', 'Updated by refreshed installer %s in %s'));
  writeFileSync(f.downloadLog, '');
  const result = f.manager();
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /Updated by refreshed installer 2.0.0/);
  assert.equal(f.log(f.downloadLog), `${githubBase}/aow-install.sh\n${githubDownloads}/2.0.0/SHA256SUMS\n${githubDownloads}/2.0.0/aow-${platform}.tar.gz\n`);
  assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/2.0.0');
  assert.equal(readlinkSync(join(f.runtime, 'active/terminald')), '../releases/1.0.0');
  assert.equal(readlinkSync(join(f.userBin, 'aow-cli')), join(f.runtime, 'releases/2.0.0/bin/aow-cli'));
  assert.equal(readFileSync(config, 'utf8'), 'AOW_SERVER_PORT=8283\n');
  assert.equal(readFileSync(pin, 'utf8'), 'keep-pin');
  assert.doesNotMatch(f.log(f.serviceLog), /terminald/);
  f.assertClean();
});

test('update version bypasses latest and repository overrides persist', t => {
  const f = fixture(t);
  f.pack('1.0.0');
  assert.equal(f.run().status, 0);
  f.pack('2.0.0');
  writeFileSync(f.downloadLog, '');
  const updated = f.manager(['update', '--version', '2.0.0', '--repo', 'override/aow'], {
    AOW_RELEASE_REPOSITORY: 'environment/aow',
  });
  assert.equal(updated.status, 0, updated.stdout + updated.stderr);
  assert.doesNotMatch(f.log(f.downloadLog), /environment/);
  const saved = () => JSON.parse(f.log(join(f.runtime, 'update.json'))).repository;
  assert.equal(saved(), 'override/aow');
  assert.equal(f.manager(['update', '--version', '1.0.0'], { AOW_RELEASE_REPOSITORY: 'environment/aow' }).status, 0);
  assert.equal(saved(), 'environment/aow');
  writeFileSync(f.downloadLog, '');
  assert.equal(f.manager(['update', '--version', '2.0.0']).status, 0);
  assert.match(f.log(f.downloadLog), /github.com\/environment\/aow\/releases\/download\/2.0.0/);
  f.assertClean();
});

test('installation and updates remember a custom command directory and help has no side effects', t => {
  const f = fixture(t);
  f.pack('1.0.0');
  const customBin = join(f.home, 'custom bin');
  const installed = f.run([], { AOW_USER_BIN_DIR: customBin });
  assert.equal(installed.status, 0, installed.stdout + installed.stderr);
  assert.deepEqual(JSON.parse(f.log(join(f.runtime, 'update.json'))), { repository: 'yorkart/aow', user_bin_dir: customBin });
  writeFileSync(f.downloadLog, '');
  const before = f.log(f.serviceLog);
  for (const args of [[], ['--help'], ['update', '--help']]) {
    const help = f.manager(args, { FAIL_DOWNLOAD: '1', FAIL_MANAGER: '1' }, undefined, customBin);
    assert.equal(help.status, 0, help.stderr);
    assert.match(help.stdout, /Usage: aow update/);
  }
  assert.equal(f.log(f.downloadLog), '');
  assert.equal(f.log(f.serviceLog), before);
  f.pack('2.0.0');
  const result = f.manager(['update'], {}, undefined, customBin);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(existsSync(join(f.userBin, 'aow')), false);
  assert.equal(readlinkSync(join(customBin, 'aow-cli')), join(f.runtime, 'releases/2.0.0/bin/aow-cli'));
  f.assertClean();
});

for (const failure of ['download', 'empty installer', 'malformed installer', 'corrupt package', 'server restart']) {
  test(`update failure (${failure}) does not persist the replacement repository and cleans temporary files`, t => {
    const f = fixture(t);
    f.pack('1.0.0');
    assert.equal(f.run().status, 0);
    const before = readFileSync(join(f.runtime, 'update.json'));
    const archive = f.pack('2.0.0');
    if (failure === 'empty installer') writeFileSync(join(f.downloads, 'aow-install.sh'), '');
    if (failure === 'malformed installer') writeFileSync(join(f.downloads, 'aow-install.sh'), 'if\n');
    if (failure === 'corrupt package') writeFileSync(archive, 'broken archive');
    const extra = failure === 'download' ? { FAIL_DOWNLOAD: '1' }
      : failure === 'server restart' ? { FAIL_SERVER_RESTART: '1' } : {};
    const result = f.manager(['update', '--repo', 'replacement/aow'], extra);
    assert.notEqual(result.status, 0);
    assert.deepEqual(readFileSync(join(f.runtime, 'update.json')), before);
    assert.equal(readlinkSync(join(f.runtime, 'active/server')), '../releases/1.0.0');
    f.assertClean();
  });
}

test('aow update only restarts terminald after lowercase y at its controlling terminal', t => {
  const f = fixture(t);
  f.pack('1.0.0');
  assert.equal(f.run().status, 0);
  f.pack('2.0.0');
  const skipped = f.manager(['update'], {}, 'n\n');
  assert.equal(skipped.status, 0, skipped.stdout + skipped.stderr);
  assert.equal(existsSync(join(f.runtime, 'active/terminald')), false);
  writeFileSync(f.serviceLog, '');
  const accepted = f.manager(['update'], {}, 'y\n');
  assert.equal(accepted.status, 0, accepted.stdout + accepted.stderr);
  assert.equal(readlinkSync(join(f.runtime, 'active/terminald')), '../releases/2.0.0');
  const services = f.log(f.serviceLog);
  assert.ok(services.indexOf('restart aow-server') < services.indexOf('restart aow-terminald'));
  f.assertClean();
});

for (const kind of ['file', 'symlink']) {
  test(`installation preserves an unrelated aow ${kind}`, t => {
    const f = fixture(t);
    f.pack();
    const custom = join(f.temp, 'other-aow');
    write(custom, 'custom application');
    const command = join(f.userBin, 'aow');
    mkdirSync(f.userBin, { recursive: true });
    if (kind === 'symlink') symlinkSync(custom, command);
    else copyFileSync(custom, command);
    assert.notEqual(f.run().status, 0);
    assert.equal(readFileSync(command, 'utf8'), 'custom application');
    assert.equal(existsSync(join(f.runtime, 'latest')), false);
    assert.equal(f.log(f.serviceLog), '--user show-environment\n');
    f.assertClean();
  });
}

test('update rejects unknown commands, invalid versions and repositories before downloading', t => {
  const f = fixture(t);
  f.pack();
  assert.equal(f.run().status, 0);
  writeFileSync(f.downloadLog, '');
  const before = f.log(f.serviceLog);
  for (const args of [['unknown'], ['update', '--version'],
    ['update', '--version', '../escape'], ['update', '--version', 'latest'],
    ['update', '--repo', 'file:///tmp'], ['update', '--repo', 'owner/repo?query=1']]) {
    assert.notEqual(f.manager(args).status, 0, JSON.stringify(args));
  }
  assert.equal(f.log(f.downloadLog), '');
  assert.equal(f.log(f.serviceLog), before);
  f.assertClean();
});
