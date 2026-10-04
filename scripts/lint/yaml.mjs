import { spawnSync } from 'node:child_process';
import { repositoryFiles } from './files.mjs';

const files = repositoryFiles().filter(file => /\.ya?ml$/.test(file));
const result = spawnSync('yamllint', ['--strict', '--', ...files], { stdio: 'inherit' });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
