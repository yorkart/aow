import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';

export function repositoryFiles() {
  return [...new Set(execFileSync('git', ['ls-files', '--cached', '--others', '--exclude-standard', '-z'], {
    encoding: 'utf8',
  }).split('\0').filter(file => file && existsSync(file)))];
}
