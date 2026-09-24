import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// Test transport only: a local directory emulates immutable GitHub release assets.
export function publishTestRelease(directory, archive, version, platform, repository = 'yorkart/aow') {
  const release = join(directory, 'releases', version);
  mkdirSync(release, { recursive: true });
  const payload = join(release, `aow-${platform}.tar.gz`);
  copyFileSync(archive, payload);
  const installer = readFileSync(new URL('../install-release.sh', import.meta.url), 'utf8')
    .replace(/^DEFAULT_REPOSITORY=.*$/m, `DEFAULT_REPOSITORY='${repository}'`)
    .replace(/^DEFAULT_GITHUB_VERSION=.*$/m, `DEFAULT_GITHUB_VERSION='${version}'`);
  writeFileSync(join(release, 'aow-install.sh'), installer);
  writeFileSync(join(directory, 'aow-install.sh'), installer);
  writeFileSync(join(directory, 'latest'), version);
  const sums = readdirSync(release).filter(name => name.endsWith('.tar.gz') || name === 'aow-install.sh')
    .map(name => `${createHash('sha256').update(readFileSync(join(release, name))).digest('hex')}  ${name}`);
  writeFileSync(join(release, 'SHA256SUMS'), `${sums.join('\n')}\n`);
  return payload;
}
