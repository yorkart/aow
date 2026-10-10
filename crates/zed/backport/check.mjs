import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const manifest = JSON.parse(await readFile(new URL('./sources.json', import.meta.url), 'utf8'));
const [upstream, target] = process.argv.slice(2);
if (!upstream) throw new Error('Usage: node crates/zed/backport/check.mjs /path/to/zed [new-commit]');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const git = args => {
  const result = spawnSync('git', ['-C', upstream, ...args], { maxBuffer: 32 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(result.stderr.toString());
  return result.stdout;
};
const paths = new Set(['Cargo.toml', 'Cargo.lock']);
for (const file of manifest.files) {
  const bytes = await readFile(path.join(root, file.local));
  if (hash(bytes) !== file.local_sha256) throw new Error(`Unrecorded local change: ${file.local}`);
  for (const source of file.sources) {
    paths.add(source.path);
    if (hash(git(['show', `${manifest.commit}:${source.path}`])) !== source.sha256) throw new Error(`Baseline mismatch: ${source.path}`);
  }
  if (file.patch) await readFile(new URL(file.patch, import.meta.url));
}
process.stdout.write(`Verified ${manifest.files.length} file mappings at ${manifest.commit}\n`);
if (target) {
  const commit = git(['rev-parse', '--verify', `${target}^{commit}`]).toString().trim();
  process.stdout.write(git(['diff', '--stat', manifest.commit, commit, '--', ...paths]));
  process.stdout.write(`Review source changes with: git -C <zed> diff ${manifest.commit} ${commit} -- <mapped source paths>\n`);
}
