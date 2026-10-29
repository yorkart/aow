import { isUtf8 } from 'node:buffer';
import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import { lstat, mkdir, open, readdir, realpath, writeFile } from 'node:fs/promises';
import { join, resolve, sep } from 'node:path';
import { HttpError } from '../errors.js';

export interface PackageFile { path: string; kind: 'file'; encoding: 'utf8' | 'base64'; content: string; executable: boolean }
export interface PackageDirectory { path: string; kind: 'directory' }
export type PackageEntry = PackageFile | PackageDirectory;
export const MAX_FILE_BYTES = 2 * 1024 * 1024;
export const MAX_PACKAGE_BYTES = 20 * 1024 * 1024;
export const MAX_ENTRIES = 1000;

export function packagePath(value: unknown): string {
  if (typeof value !== 'string' || !value || value.length > 240 || /[\\\p{Cc}]/u.test(value) || value.split('/').some(part => !part || !part.trim() || part === '.' || part === '..')) {
    throw new HttpError(422, 'invalid_package_path', '文件路径须为包内相对路径，不能包含空路径、反斜杠或 ..。');
  }
  return value;
}

export function bytes(entry: PackageFile): Buffer {
  return Buffer.from(entry.content, entry.encoding === 'base64' ? 'base64' : 'utf8');
}

export function fileEntry(path: string, content: Buffer, executable = false): PackageFile {
  const encoding = isUtf8(content) && !content.includes(0) ? 'utf8' : 'base64';
  return { path, kind: 'file', encoding, content: content.toString(encoding), executable };
}

export function validatePackage(input: unknown): PackageEntry[] {
  if (!Array.isArray(input) || !input.length || input.length > MAX_ENTRIES) throw new HttpError(422, 'invalid_package', `技能包须包含 1–${MAX_ENTRIES} 个文件或目录。`);
  const entries: PackageEntry[] = [];
  const paths = new Map<string, string>();
  let total = 0;
  for (const item of input as unknown[]) {
    if (!item || typeof item !== 'object') throw new HttpError(422, 'invalid_package', '技能包条目无效。');
    const value = item as Record<string, unknown>;
    const path = packagePath(value.path);
    if (paths.has(path)) throw new HttpError(422, 'duplicate_path', `文件路径重复：${path}`);
    paths.set(path, String(value.kind));
    if (value.kind === 'directory') { entries.push({ path, kind: 'directory' }); continue; }
    if (value.kind !== 'file' || typeof value.content !== 'string' || !['utf8', 'base64'].includes(String(value.encoding))) throw new HttpError(422, 'invalid_package', '文件内容或编码无效。');
    if (value.content.length > MAX_FILE_BYTES * 2) throw new HttpError(413, 'file_too_large', '单个文件不能超过 2 MiB。');
    if (value.encoding === 'base64' && !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value.content)) throw new HttpError(422, 'invalid_encoding', '文件的 Base64 编码无效。');
    const buffer = Buffer.from(value.content, value.encoding === 'base64' ? 'base64' : 'utf8');
    if (buffer.length > MAX_FILE_BYTES) throw new HttpError(413, 'file_too_large', '单个文件不能超过 2 MiB。');
    total += buffer.length;
    if (total > MAX_PACKAGE_BYTES) throw new HttpError(413, 'package_too_large', '技能包不能超过 20 MiB。');
    entries.push(fileEntry(path, buffer, value.executable === true));
  }
  for (const path of paths.keys()) {
    const parts = path.split('/');
    parts.pop();
    while (parts.length) {
      const parent = parts.join('/');
      if (paths.get(parent) === 'file') throw new HttpError(422, 'invalid_package_path', `文件不能同时作为目录：${parent}`);
      if (!paths.has(parent)) { paths.set(parent, 'directory'); entries.push({ path: parent, kind: 'directory' }); }
      parts.pop();
    }
  }
  const entry = entries.find(item => item.path === 'SKILL.md');
  if (!entry || entry.kind !== 'file' || entry.encoding !== 'utf8') throw new HttpError(422, 'missing_entry', '入口文件必须为 UTF-8 文本 SKILL.md，不能删除或重命名。');
  if (entries.length > MAX_ENTRIES) throw new HttpError(413, 'package_too_large', '技能包目录层级过多。');
  return entries.sort((a, b) => a.path.localeCompare(b.path, 'en'));
}

export function packageHash(entries: PackageEntry[]): string {
  const hash = createHash('sha256');
  for (const entry of entries) {
    hash.update(JSON.stringify([entry.path, entry.kind, entry.kind === 'file' && entry.executable]));
    if (entry.kind === 'file') hash.update(bytes(entry));
    hash.update('\0');
  }
  return hash.digest('hex');
}

export async function writePackage(directory: string, entries: PackageEntry[]): Promise<void> {
  await mkdir(directory, { recursive: false, mode: 0o700 });
  for (const entry of entries.filter(item => item.kind === 'directory').sort((a, b) => a.path.length - b.path.length)) await mkdir(join(directory, entry.path), { recursive: true, mode: 0o700 });
  for (const entry of entries) if (entry.kind === 'file') await writeFile(join(directory, entry.path), bytes(entry), { flag: 'wx', mode: entry.executable ? 0o500 : 0o400 });
}

/** No symlinks or special files, including links pointing elsewhere in a package. */
export async function readPackage(directory: string, boundary = directory): Promise<PackageEntry[]> {
  const root = await realpath(boundary);
  const base = await realpath(directory);
  if (base !== root && !base.startsWith(root + sep)) throw new HttpError(422, 'outside_project', '技能目录超出项目工作目录。');
  const entries: PackageEntry[] = [];
  let total = 0;
  const visit = async (relative: string) => {
    const path = join(base, relative);
    const info = await lstat(path);
    if (info.isSymbolicLink()) throw new HttpError(422, 'unsafe_package', '技能包包含符号链接，请改为实际文件后再导入。');
    const canonical = await realpath(path);
    if (canonical !== base && !canonical.startsWith(base + sep)) throw new HttpError(422, 'unsafe_package', '文件超出技能包目录。');
    if (relative) {
      packagePath(relative);
      if (entries.length >= MAX_ENTRIES) throw new HttpError(413, 'package_too_large', '技能包文件数量过多。');
    }
    if (info.isDirectory()) {
      if (relative) entries.push({ path: relative, kind: 'directory' });
      for (const child of await readdir(path)) await visit(relative ? `${relative}/${child}` : child);
    } else if (info.isFile()) {
      if (info.size > MAX_FILE_BYTES || total + info.size > MAX_PACKAGE_BYTES) throw new HttpError(413, 'package_too_large', '技能包超过文件或总大小限制。');
      const file = await open(resolve(path), constants.O_RDONLY | constants.O_NOFOLLOW);
      try {
        const content = await file.readFile();
        total += content.length;
        if (content.length > MAX_FILE_BYTES || total > MAX_PACKAGE_BYTES) throw new HttpError(413, 'package_too_large', '技能包超过文件或总大小限制。');
        entries.push(fileEntry(relative, content, Boolean(info.mode & 0o111)));
      } finally { await file.close(); }
    } else throw new HttpError(422, 'unsafe_package', '技能包仅支持普通文件和目录。');
  };
  await visit('');
  return validatePackage(entries);
}
