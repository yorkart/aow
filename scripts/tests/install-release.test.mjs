import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { replaceSymlink } from '../replace-symlink.mjs';

function temporary(t) {
  const path = mkdtempSync(join(tmpdir(), 'aow-install-'));
  t.after(() => rmSync(path, { recursive: true, force: true }));
  return path;
}

function write(path, content, executable = false) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content, { mode: executable ? 0o755 : 0o644 });
}

test('atomic replacement handles directory links and preserves old links on failure', t => {
  const temp = temporary(t);
  mkdirSync(join(temp, 'old'));
  mkdirSync(join(temp, 'new'));
  const current = join(temp, 'current');
  const pending = join(temp, 'pending');
  symlinkSync(join(temp, 'old'), current);
  symlinkSync(join(temp, 'new'), pending);
  replaceSymlink(pending, current);
  assert.equal(readlinkSync(current), join(temp, 'new'));
  assert.equal(existsSync(join(temp, 'old', 'pending')), false);
  symlinkSync(join(temp, 'missing'), pending);
  assert.throws(() => replaceSymlink(pending, current), { code: 'ENOENT' });
  assert.equal(readlinkSync(current), join(temp, 'new'));
  unlinkSync(pending);
  if (process.getuid() !== 0) {
    symlinkSync(join(temp, 'old'), pending);
    chmodSync(temp, 0o555);
    try {
      assert.throws(() => replaceSymlink(pending, current), { code: 'EACCES' });
      assert.equal(readlinkSync(current), join(temp, 'new'));
    } finally {
      chmodSync(temp, 0o700);
    }
  }
});

test('an unmanaged executable is preserved', t => {
  const temp = temporary(t);
  const current = join(temp, 'aow-cli');
  write(current, 'custom executable');
  write(join(temp, 'new'), 'new executable');
  symlinkSync(join(temp, 'new'), join(temp, 'pending'));
  assert.throws(() => replaceSymlink(join(temp, 'pending'), current), /unmanaged/);
  assert.equal(readFileSync(current, 'utf8'), 'custom executable');
});

test('the helper runs through symlinked directories and script paths', t => {
  const temp = temporary(t);
  const real = join(temp, 'real');
  const alias = join(temp, 'alias');
  mkdirSync(real);
  copyFileSync(new URL('../replace-symlink.mjs', import.meta.url), join(real, 'helper.mjs'));
  symlinkSync(real, alias);
  symlinkSync(join(real, 'helper.mjs'), join(temp, 'helper-link.mjs'));
  const target = join(temp, 'target');
  const pending = join(temp, 'pending');
  const current = join(temp, 'current');
  write(target, 'payload');
  for (const script of [join(real, 'helper.mjs'), join(alias, 'helper.mjs'), join(temp, 'helper-link.mjs')]) {
    symlinkSync(target, pending);
    const result = spawnSync(process.execPath, [script, pending, current], { stdio: 'inherit', timeout: 5000 });
    assert.ifError(result.error);
    assert.equal(result.status, 0);
    assert.equal(existsSync(pending), false, script);
    assert.equal(readlinkSync(current), target);
    rmSync(current);
  }
});
