#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, copyFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const platforms = ['linux-x86_64', 'linux-aarch64', 'macos-x86_64', 'macos-aarch64'];
const validVersion = value => /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(value) && value !== 'latest';

export async function prepareGitHubRelease({ version, revision, repository, inputDir, outputDir }) {
  if (!version || !validVersion(version)) throw new Error('Invalid release version');
  if (!/^[a-f0-9]{40}$/.test(revision ?? '')) throw new Error('Expected a full Git commit SHA');
  if (!/^[A-Za-z0-9_-][A-Za-z0-9_-]*\/[A-Za-z0-9_.-]+$/.test(repository ?? '')) throw new Error('Expected repository OWNER/REPO');
  if (!inputDir || !outputDir) throw new Error('Input and output directories are required');
  outputDir = resolve(outputDir);
  if (existsSync(outputDir)) throw new Error(`Output already exists: ${outputDir}`);
  mkdirSync(dirname(outputDir), { recursive: true });
  const stage = mkdtempSync(`${outputDir}.pending-`);
  try {
    const assets = [];
    for (const platform of platforms) {
      const name = `aow-${platform}.tar.gz`;
      const source = join(inputDir, name);
      if (!lstatSync(source).isFile()) throw new Error(`Expected a regular package: ${name}`);
      const entries = execFileSync('tar', ['-tzf', source], { encoding: 'utf8' }).trim().split('\n');
      const manifests = entries.filter(entry => entry === './manifest.json' || entry === 'manifest.json');
      if (manifests.length !== 1) throw new Error(`Expected one root manifest in ${name}`);
      const manifest = JSON.parse(execFileSync('tar', ['-xOzf', source, '--', manifests[0]], { encoding: 'utf8' }));
      const [os, arch] = platform.split('-');
      const target = `${arch}-${os === 'linux' ? 'unknown-linux-musl' : 'apple-darwin'}`;
      if (manifest.release_id !== version || manifest.git_revision !== revision
          || manifest.git_dirty !== false || manifest.reused_build_artifacts !== false
          || manifest.platform?.os !== os || manifest.platform?.arch !== arch
          || manifest.platform?.target !== target
          || (os === 'linux' && (manifest.platform.libc !== 'musl' || manifest.platform.linkage !== 'static'))) {
        throw new Error(`Package metadata does not match the clean ${version} build at ${revision}: ${name}`);
      }
      copyFileSync(source, join(stage, name));
      assets.push(name);
    }
    let installer = readFileSync(join(root, 'scripts/install-release.sh'), 'utf8');
    for (const [key, value] of Object.entries({
      DEFAULT_REPOSITORY: repository,
      DEFAULT_GITHUB_VERSION: version,
    })) {
      const assignment = new RegExp(`^${key}=.*$`, 'gm');
      if ([...installer.matchAll(assignment)].length !== 1) throw new Error(`Expected one ${key} assignment`);
      installer = installer.replace(assignment, `${key}='${value}'`);
    }
    writeFileSync(join(stage, 'aow-install.sh'), installer, { mode: 0o755 });
    execFileSync('bash', ['-n', join(stage, 'aow-install.sh')]);
    assets.push('aow-install.sh');
    const sums = [];
    for (const name of assets) {
      const hash = createHash('sha256');
      for await (const chunk of createReadStream(join(stage, name))) hash.update(chunk);
      sums.push(`${hash.digest('hex')}  ${name}`);
    }
    writeFileSync(join(stage, 'SHA256SUMS'), `${sums.join('\n')}\n`);
    renameSync(stage, outputDir);
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args.length !== 5) {
    console.error('Usage: prepare-github-release.mjs VERSION COMMIT_SHA OWNER/REPO INPUT_DIR OUTPUT_DIR');
    process.exitCode = 1;
  } else {
    const [version, revision, repository, inputDir, outputDir] = args;
    await prepareGitHubRelease({ version, revision, repository, inputDir, outputDir });
    console.log(resolve(outputDir));
  }
}
