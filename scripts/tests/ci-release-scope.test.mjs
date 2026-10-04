import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { requiresNativePackages } from '../ci/release-scope.mjs';

test('main and manual runs always build packages; ordinary PR changes run the tests', () => {
  assert.equal(requiresNativePackages('push'), true);
  assert.equal(requiresNativePackages('workflow_dispatch'), true);
  assert.equal(requiresNativePackages('pull_request', [
    'crates/server/src/main.rs', 'frontend/src/App.tsx', 'docs/usage.md',
  ]), false);
});

test('release tooling, native dependencies and build configuration require every platform', () => {
  for (const path of [
    'scripts/package-release.sh', 'scripts/tests/macos-release.test.mjs',
    'packaging/bin/aow', '.github/workflows/release.yml', '.cargo/config.toml',
    'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'rust-toolchain',
    'crates/server/Cargo.toml', 'crates/terminald/build.rs', 'crates/macos-log/src/os_log.c',
    'frontend/package.json', 'frontend/package-lock.json', 'vt-worker/bin/vt-worker.mjs',
  ]) {
    assert.equal(requiresNativePackages('pull_request', [path]), true, path);
  }
});

for (const change of ['delete', 'rename']) {
  test(`PR scope includes earlier commits and ${change}d files in the merge diff`, t => {
    const directory = mkdtempSync(join(tmpdir(), 'aow-ci-scope-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const git = (...args) => execFileSync('git', args, { cwd: directory, stdio: 'pipe' });
    git('init', '-b', 'main');
    git('config', 'user.name', 'Test');
    git('config', 'user.email', 'test@example.invalid');
    mkdirSync(join(directory, 'scripts'));
    writeFileSync(join(directory, 'scripts/old release.sh'), 'old');
    git('add', '.'); git('commit', '-m', 'base');
    git('checkout', '-b', 'feature');
    if (change === 'delete') git('rm', 'scripts/old release.sh');
    else git('mv', 'scripts/old release.sh', 'old release.txt');
    git('commit', '-m', 'remove release script');
    writeFileSync(join(directory, 'README.md'), 'documentation');
    git('add', '.'); git('commit', '-m', 'documentation');
    git('checkout', 'main'); git('merge', '--no-ff', 'feature', '-m', 'merge');
    const output = join(directory, 'output');
    const script = fileURLToPath(new URL('../ci/release-scope.mjs', import.meta.url));
    const result = spawnSync(process.execPath, [script], { cwd: directory, encoding: 'utf8',
      env: { ...process.env, GITHUB_EVENT_NAME: 'pull_request', GITHUB_OUTPUT: output } });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(readFileSync(output, 'utf8'), 'native-packages=true\n');
  });
}
