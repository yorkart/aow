import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { platforms, prepareGitHubRelease } from '../prepare-github-release.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const revision = 'a'.repeat(40);
const version = 'v2026.09.24-123.1-aaaaaaaaaaaa';

function fixture(t, customize = () => {}) {
  const temp = mkdtempSync(join(tmpdir(), 'aow-github-release-'));
  t.after(() => rmSync(temp, { recursive: true, force: true }));
  const inputDir = join(temp, 'input');
  const outputDir = join(temp, 'release assets');
  mkdirSync(inputDir);
  for (const platform of platforms) {
    const bundle = join(temp, platform);
    mkdirSync(bundle);
    const [os, arch] = platform.split('-');
    const manifest = {
      release_id: version, git_revision: revision, git_dirty: false, reused_build_artifacts: false,
      platform: { os, arch, target: `${arch}-${os === 'linux' ? 'unknown-linux-musl' : 'apple-darwin'}`,
        ...(os === 'linux' ? { libc: 'musl', linkage: 'static' } : { linkage: 'dynamic' }) },
    };
    customize(manifest, platform);
    writeFileSync(join(bundle, 'manifest.json'), JSON.stringify(manifest));
    const packed = spawnSync('tar', ['-czf', join(inputDir, `aow-${platform}.tar.gz`), '-C', bundle, '.']);
    assert.equal(packed.status, 0, packed.stderr?.toString());
  }
  const prepare = () => prepareGitHubRelease({ version, revision, repository: 'yorkart/aow', inputDir, outputDir });
  const tools = join(temp, 'tools');
  mkdirSync(tools);
  const state = join(temp, 'state.json');
  const log = join(temp, 'gh.log');
  writeFileSync(join(tools, 'gh'), `#!${process.execPath}
const fs = require('node:fs');
const args = process.argv.slice(2);
fs.appendFileSync(process.env.GH_TEST_LOG, JSON.stringify(args) + '\\n');
if (args[0] === 'api') { console.log(process.env.GH_TEST_MAIN); process.exit(0); }
const path = process.env.GH_TEST_STATE;
const release = fs.existsSync(path) ? JSON.parse(fs.readFileSync(path)) : null;
if (args[1] === 'view') {
  if (!release) process.exit(1);
  console.log(JSON.stringify(release));
} else if (args[1] === 'create') {
  if (!args.includes('--draft') || release) process.exit(99);
  fs.writeFileSync(path, JSON.stringify({ isDraft: true, targetCommitish: args[args.indexOf('--target') + 1] }));
} else if (args[1] === 'upload') {
  if (!release?.isDraft) process.exit(98);
  if (process.env.GH_TEST_FAIL_UPLOAD === '1') process.exit(2);
} else if (args[1] === 'edit') {
  if (!args.includes('--draft=false') || !args.includes('--latest')) process.exit(97);
  fs.writeFileSync(path, JSON.stringify({ ...release, isDraft: false }));
} else process.exit(96);
`, { mode: 0o755 });
  const publish = (extra = {}) => spawnSync('bash', [join(root, 'scripts/publish-github-release.sh'), version, revision, outputDir], {
    encoding: 'utf8', timeout: 15000,
    env: { ...process.env, PATH: `${tools}:${process.env.PATH}`, GH_REPO: 'yorkart/aow',
      GH_TEST_STATE: state, GH_TEST_LOG: log, GH_TEST_MAIN: revision, ...extra },
  });
  const calls = () => existsSync(log) ? readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map(line => JSON.parse(line)) : [];
  return { temp, inputDir, outputDir, prepare, publish, state, log, calls };
}

test('prepare aggregates four byte-identical native packages, a pinned installer and SHA256 checksums', async t => {
  const f = fixture(t);
  await f.prepare();
  assert.equal(readdirSync(f.outputDir).length, 6);
  for (const platform of platforms) {
    const name = `aow-${platform}.tar.gz`;
    assert.deepEqual(readFileSync(join(f.inputDir, name)), readFileSync(join(f.outputDir, name)));
  }
  const installer = readFileSync(join(f.outputDir, 'aow-install.sh'), 'utf8');
  assert.ok(installer.includes(`DEFAULT_GITHUB_VERSION='${version}'`));
  assert.ok(installer.includes("DEFAULT_REPOSITORY='yorkart/aow'"));
  for (const line of readFileSync(join(f.outputDir, 'SHA256SUMS'), 'utf8').trim().split('\n')) {
    const [hash, name] = line.split('  ');
    assert.equal(hash, createHash('sha256').update(readFileSync(join(f.outputDir, name))).digest('hex'));
  }
  await assert.rejects(f.prepare(), /Output already exists/);
});

for (const [field, value] of [['release_id', 'other'], ['git_revision', 'b'.repeat(40)], ['git_dirty', true],
  ['reused_build_artifacts', true], ['platform', { os: 'linux', arch: 'x86_64', target: 'x86_64-unknown-linux-gnu' }]]) {
  test(`prepare rejects a mismatched ${field} without exposing a partial release`, async t => {
    const f = fixture(t, (manifest, platform) => { if (platform === 'macos-aarch64') manifest[field] = value; });
    await assert.rejects(f.prepare(), /Package metadata does not match/);
    assert.equal(existsSync(f.outputDir), false);
    assert.equal(readdirSync(f.temp).some(name => name.includes('.pending-')), false);
  });
}

test('prepare requires all four platforms', async t => {
  const f = fixture(t);
  rmSync(join(f.inputDir, 'aow-macos-x86_64.tar.gz'));
  await assert.rejects(f.prepare(), /ENOENT/);
  assert.equal(existsSync(f.outputDir), false);
});

test('publication uploads all assets to a draft before making it latest; published assets cannot be replaced', async t => {
  const f = fixture(t);
  await f.prepare();
  const result = f.publish();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.deepEqual(f.calls().map(args => args.slice(0, 2)), [
    ['api', 'repos/yorkart/aow/git/ref/heads/main'], ['release', 'view'], ['release', 'create'], ['release', 'upload'], ['release', 'edit'],
  ]);
  assert.equal(f.calls().find(args => args[1] === 'upload').length, 10);
  writeFileSync(f.log, '');
  const repeated = f.publish();
  assert.notEqual(repeated.status, 0);
  assert.match(repeated.stderr, /Refusing to replace/);
  assert.equal(f.calls().some(args => ['upload', 'edit'].includes(args[1])), false);
});

test('an interrupted asset upload stays a draft and the same commit can resume it', async t => {
  const f = fixture(t);
  await f.prepare();
  assert.notEqual(f.publish({ GH_TEST_FAIL_UPLOAD: '1' }).status, 0);
  assert.equal(JSON.parse(readFileSync(f.state)).isDraft, true);
  assert.equal(f.calls().some(args => args[1] === 'edit'), false);
  writeFileSync(f.log, '');
  const result = f.publish();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(f.calls().some(args => args[1] === 'create'), false);
  assert.equal(JSON.parse(readFileSync(f.state)).isDraft, false);
});

test('publication rejects another commit draft, stale main and modified assets', async t => {
  const f = fixture(t);
  await f.prepare();
  writeFileSync(f.state, JSON.stringify({ isDraft: true, targetCommitish: 'b'.repeat(40) }));
  assert.notEqual(f.publish().status, 0);
  assert.equal(f.calls().some(args => ['upload', 'edit'].includes(args[1])), false);
  writeFileSync(f.log, '');
  const stale = f.publish({ GH_TEST_MAIN: 'c'.repeat(40) });
  assert.equal(stale.status, 0, stale.stderr);
  assert.match(stale.stdout, /Skipping publication/);
  assert.equal(f.calls().length, 1);
  writeFileSync(f.log, '');
  writeFileSync(join(f.outputDir, 'aow-install.sh'), 'changed after validation');
  assert.notEqual(f.publish().status, 0);
  assert.deepEqual(f.calls(), []);
});
