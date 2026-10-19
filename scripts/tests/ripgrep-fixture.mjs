import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));

export function vendorRipgrepFixture(repo, binary) {
  mkdirSync(join(repo, 'scripts'), { recursive: true });
  copyFileSync(join(root, 'scripts/ripgrep.mjs'), join(repo, 'scripts/ripgrep.mjs'));
  const vendor = join(repo, 'vendor/ripgrep');
  mkdirSync(vendor, { recursive: true });
  const temp = mkdtempSync(join(vendor, '.fixture-'));
  try {
    const directory = 'ripgrep-fixture';
    mkdirSync(join(temp, directory));
    if (binary) copyFileSync(binary, join(temp, directory, 'rg'));
    else writeFileSync(join(temp, directory, 'rg'), '#!/bin/sh\necho "ripgrep fixture"\n', { mode: 0o755 });
    writeFileSync(join(temp, directory, 'COPYING'), 'Fixture license');
    const archive = join(vendor, 'fixture.tar.gz');
    execFileSync('tar', ['-czf', archive, '-C', temp, directory]);
    const sha256 = createHash('sha256').update(readFileSync(archive)).digest('hex');
    const artifacts = Object.fromEntries(['x86_64-unknown-linux-musl', 'aarch64-unknown-linux-musl', 'x86_64-apple-darwin', 'aarch64-apple-darwin']
      .map(target => [target, { directory, archive: 'fixture.tar.gz', sha256 }]));
    writeFileSync(join(vendor, 'manifest.json'), JSON.stringify({ version: 'fixture', artifacts }));
  } finally { rmSync(resolve(temp), { recursive: true, force: true }); }
}
