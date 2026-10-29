import { createCipheriv, createDecipheriv, randomBytes } from 'node:crypto';
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { DatabaseSync } from 'node:sqlite';
import { HttpError } from '../errors.js';
import type { Credentials } from './types.js';

/** The SQLite database contains ciphertext; the instance key lives in ~/.orbit. */
export class CredentialVault {
  constructor(private directory: string, private database: DatabaseSync) {}

  private key(): Buffer {
    mkdirSync(this.directory, { recursive: true, mode: 0o700 });
    const path = join(this.directory, 'credentials.key');
    try {
      const key = readFileSync(path);
      if (key.length !== 32) throw new Error();
      return key;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw new HttpError(503, 'credential_store_unavailable', '凭据存储不可用，请检查 Orbit 数据目录。');
      if (this.database.prepare('SELECT 1 FROM connectors WHERE credentials IS NOT NULL UNION ALL SELECT 1 FROM connector_auth_sessions WHERE credentials IS NOT NULL UNION ALL SELECT 1 FROM agent_secrets LIMIT 1').get()) {
        throw new HttpError(503, 'credential_key_missing', '凭据加密密钥缺失，请恢复此 Orbit 实例的 credentials.key。');
      }
      const key = randomBytes(32);
      let file: number;
      try { file = openSync(path, 'wx', 0o600); }
      catch (error) { if ((error as NodeJS.ErrnoException).code === 'EEXIST') return this.key(); throw new HttpError(503, 'credential_store_unavailable', '无法创建凭据加密密钥。'); }
      try { writeFileSync(file, key); } finally { closeSync(file); }
      return key;
    }
  }

  seal(value: object, scope: string): string {
    const nonce = randomBytes(12);
    const cipher = createCipheriv('aes-256-gcm', this.key(), nonce);
    cipher.setAAD(Buffer.from(scope));
    const content = Buffer.concat([cipher.update(JSON.stringify(value), 'utf8'), cipher.final()]);
    return [nonce, cipher.getAuthTag(), content].map(item => item.toString('base64')).join('.');
  }

  open<T = Credentials>(value: string, scope: string): T {
    try {
      const [nonce, tag, content] = value.split('.').map(part => Buffer.from(part, 'base64'));
      const cipher = createDecipheriv('aes-256-gcm', this.key(), nonce);
      cipher.setAAD(Buffer.from(scope)); cipher.setAuthTag(tag);
      return JSON.parse(Buffer.concat([cipher.update(content), cipher.final()]).toString('utf8')) as T;
    } catch { throw new HttpError(503, 'credential_store_unavailable', '无法解密连接凭据，请检查 Orbit 实例密钥。'); }
  }
}
