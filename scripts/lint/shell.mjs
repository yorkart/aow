import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { repositoryFiles } from './files.mjs';

const files = repositoryFiles().filter(file => file.endsWith('.sh') || file.startsWith('packaging/bin/'));

for (const file of files) {
  const interpreter = /^#!.*\bbash\b/.test(readFileSync(file, 'utf8')) ? 'bash' : 'sh';
  const result = spawnSync(interpreter, ['-n', file], { stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
const result = spawnSync('shellcheck', ['--severity=style', '--', ...files], { stdio: 'inherit' });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
