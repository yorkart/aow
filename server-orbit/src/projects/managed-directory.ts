import { constants, realpathSync } from 'node:fs';
import { access, lstat, mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { HttpError } from '../errors.js';

export class ManagedProjectDirectories {
  private root: string;
  constructor(stateDir: string) { this.root = realpathSync(stateDir); }
  path(org: string, project: string) {
    if (![org, project].every(value => /^[A-Za-z0-9-]+$/.test(value))) throw new HttpError(422, 'invalid_project', '项目目录标识无效。');
    return join(this.root, 'projects', org, project, '_default');
  }
  async prepare(org: string, project: string, create: boolean) {
    const directory = this.path(org, project);
    try {
      let cursor = this.root;
      for (const part of ['projects', org, project, '_default']) {
        cursor = join(cursor, part);
        if (create) await mkdir(cursor, { mode: 0o700 }).catch((error: NodeJS.ErrnoException) => { if (error.code !== 'EEXIST') throw error; });
        const info = await lstat(cursor);
        if (!info.isDirectory() || info.isSymbolicLink()) throw new Error();
      }
      await access(directory, constants.R_OK | constants.W_OK | constants.X_OK);
      return directory;
    } catch { throw new HttpError(422, 'workspace_unavailable', 'Orbit 默认工作目录丢失、被替换或无法访问，请修复目录或选择其他本地目录。'); }
  }
}
