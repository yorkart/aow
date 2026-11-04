import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export function stageRipgrep(target, destination, vendor = join(root, 'vendor/ripgrep')) {
  const manifest = JSON.parse(readFileSync(join(vendor, 'manifest.json'), 'utf8'));
  const artifact = manifest.artifacts[target.replace(/-unknown-linux-gnu$/, '-unknown-linux-musl')];
  if (!artifact) throw new Error(`No bundled ripgrep for ${target}`);
  const archive = join(vendor, artifact.archive);
  const checksum = createHash('sha256').update(readFileSync(archive)).digest('hex');
  if (checksum !== artifact.sha256) throw new Error(`ripgrep checksum mismatch: ${archive}`);
  mkdirSync(destination, { recursive: true });
  const temporary = mkdtempSync(join(destination, '.ripgrep-'));
  try {
    execFileSync('tar', ['-xzf', archive, '-C', temporary]);
    const source = join(temporary, artifact.directory);
    mkdirSync(join(destination, 'bin'), { recursive: true });
    copyFileSync(join(source, 'rg'), join(destination, 'bin/rg'));
    chmodSync(join(destination, 'bin/rg'), 0o755);
    const licenses = join(destination, 'licenses/ripgrep');
    mkdirSync(licenses, { recursive: true });
    for (const name of readdirSync(source).filter(name => /^(COPYING|LICENSE|UNLICENSE)/.test(name))) {
      copyFileSync(join(source, name), join(licenses, name));
    }
    copyFileSync(join(vendor, 'manifest.json'), join(licenses, 'manifest.json'));
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
  return manifest.version;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [target, destination] = process.argv.slice(2);
  if (!target || !destination) throw new Error('Usage: node scripts/ripgrep.mjs TARGET DESTINATION');
  stageRipgrep(target, resolve(destination));
}
