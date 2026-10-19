import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { stageRipgrep } from '../ripgrep.mjs';

const vendor = fileURLToPath(new URL('../../vendor/ripgrep/', import.meta.url));
const manifest = JSON.parse(readFileSync(join(vendor, 'manifest.json'), 'utf8'));
test('all vendored release archives verify and contain an executable and upstream licenses', t => {
  const temp = mkdtempSync(join(tmpdir(), 'aow-rg-')); t.after(() => rmSync(temp, { recursive: true, force: true }));
  for (const [target, artifact] of Object.entries(manifest.artifacts)) {
    assert.equal(createHash('sha256').update(readFileSync(join(vendor, artifact.archive))).digest('hex'), artifact.sha256);
    const output = join(temp, target); stageRipgrep(target, output);
    assert.equal(statSync(join(output, 'bin/rg')).mode & 0o111, 0o111);
    assert.ok(existsSync(join(output, 'licenses/ripgrep/COPYING')));
    const host = `${process.arch === 'arm64' ? 'aarch64' : 'x86_64'}-${process.platform === 'darwin' ? 'apple-darwin' : 'unknown-linux-musl'}`;
    if (target === host) assert.match(execFileSync(join(output, 'bin/rg'), ['--version'], { encoding: 'utf8' }), new RegExp(`^ripgrep ${manifest.version.replaceAll('.', '\\.')}`));
  }
});
test('a changed archive fails checksum validation before extraction', t => {
  const temp = mkdtempSync(join(tmpdir(), 'aow-rg-bad-')); t.after(() => rmSync(temp, { recursive: true, force: true }));
  writeFileSync(join(temp, 'bad.tar.gz'), 'broken');
  writeFileSync(join(temp, 'manifest.json'), JSON.stringify({ version: 'broken', artifacts: { target: { archive: 'bad.tar.gz', sha256: 'wrong' } } }));
  assert.throws(() => stageRipgrep('target', join(temp, 'out'), temp), /checksum mismatch/);
  assert.equal(existsSync(join(temp, 'out')), false);
});
