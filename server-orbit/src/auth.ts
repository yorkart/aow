// Ported from crates/server/src/auth: local password login, bounded admission,
// passive status checks, absolute/idle expiry, and credential-change revocation.
import { createHash, pbkdf2, randomBytes, randomUUID, timingSafeEqual } from 'node:crypto';
import type { IncomingMessage } from 'node:http';
import type { DatabaseSync } from 'node:sqlite';
import { promisify } from 'node:util';
import type { Config } from './config.js';
import { account, transaction } from './database.js';
import { HttpError } from './errors.js';

const derive = promisify(pbkdf2);
const COOKIE = 'orbit_session';
const ITERATIONS = 600_000;

export function validUsername(value: unknown): value is string {
  return typeof value === 'string' && Buffer.byteLength(value) <= 256 && value.length > 0 && !/[\s\p{Cc}]/u.test(value);
}

function validPassword(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && Buffer.byteLength(value) <= 1024;
}

export async function setAccount(database: DatabaseSync, username: string, password: string): Promise<void> {
  if (!validUsername(username) || !validPassword(password)) throw new Error('请输入有效的用户名和密码。');
  const salt = randomBytes(16);
  const hash = await derive(password, salt, ITERATIONS, 32, 'sha256');
  transaction(database, () => {
    database.prepare(`INSERT INTO account (id, username, salt, password_hash, revision) VALUES (1, ?, ?, ?, ?)
      ON CONFLICT(id) DO UPDATE SET username = excluded.username, salt = excluded.salt,
      password_hash = excluded.password_hash, revision = excluded.revision`)
      .run(username, salt.toString('hex'), hash.toString('hex'), randomUUID());
    database.exec('DELETE FROM sessions');
  });
}

function tokenHash(token: string): string {
  return createHash('sha256').update(token).digest('hex');
}

function sessionToken(request: IncomingMessage): string | undefined {
  const value = request.headers.cookie?.split(';').map(entry => entry.trim())
    .find(entry => entry.startsWith(`${COOKIE}=`))?.slice(COOKIE.length + 1);
  return value && /^[a-f0-9]{64}$/.test(value) ? value : undefined;
}

export class Auth {
  private tokens = 10;
  private updated = Date.now();
  private active = 0;
  private failures = 0;
  private blockedUntil = 0;

  constructor(private database: DatabaseSync, private config: Config) {}

  authenticated(request: IncomingMessage, touch = false): boolean {
    const token = sessionToken(request);
    if (!token) return false;
    const now = Date.now();
    this.database.prepare(`DELETE FROM sessions WHERE created_at <= ? OR last_active <= ?
      OR account_revision != COALESCE((SELECT revision FROM account WHERE id = 1), '')`)
      .run(now - this.config.sessionMaxAge, now - this.config.sessionIdleTimeout);
    const found = this.database.prepare('SELECT token_hash FROM sessions WHERE token_hash = ?').get(tokenHash(token));
    if (found && touch) this.database.prepare('UPDATE sessions SET last_active = ? WHERE token_hash = ?').run(now, tokenHash(token));
    return Boolean(found);
  }

  status(request: IncomingMessage) {
    const configured = Boolean(account(this.database));
    return {
      configured,
      authenticated: this.authenticated(request),
      methods: [{ id: 'password', label: '用户名和密码', configured }],
      ...(!configured ? { message: '请先在服务器上运行 Orbit 账户配置脚本。' } : {}),
    };
  }

  cookie(token: string): string {
    return `${COOKIE}=${token}; Path=/; HttpOnly; SameSite=Strict${this.config.secureCookies ? '; Secure' : ''}${token ? '' : '; Max-Age=0'}`;
  }

  revoke(request: IncomingMessage): void {
    const token = sessionToken(request);
    if (token) this.database.prepare('DELETE FROM sessions WHERE token_hash = ?').run(tokenHash(token));
  }

  async login(credentials: Record<string, unknown>): Promise<string> {
    const now = Date.now();
    this.tokens = Math.min(10, this.tokens + (now - this.updated) / 6000);
    this.updated = now;
    const wait = Math.max(this.blockedUntil - now, this.tokens < 1 ? (1 - this.tokens) * 6000 : 0, this.active >= 2 ? 1000 : 0);
    if (wait > 0) throw new HttpError(429, 'login_rate_limited', '登录尝试过多，请稍后重试。', Math.ceil(wait / 1000));
    this.tokens -= 1;
    this.active += 1;
    try {
      if ((credentials.method ?? 'password') !== 'password') throw new HttpError(400, 'unsupported_method', '不支持的登录方式。');
      const { username, password } = credentials;
      if (!validUsername(username) || !validPassword(password)) throw new HttpError(400, 'invalid_credentials', '请输入有效的用户名和密码。');
      const configured = account(this.database);
      if (!configured) throw new HttpError(503, 'account_not_configured', '请先配置登录账户。');
      const hash = await derive(password, Buffer.from(configured.salt, 'hex'), ITERATIONS, 32, 'sha256');
      if (!timingSafeEqual(hash, Buffer.from(configured.password_hash, 'hex')) || username !== configured.username) {
        this.failures += 1;
        if (this.failures >= 5) this.blockedUntil = Date.now() + Math.min(60, 2 ** Math.min(6, this.failures - 5)) * 1000;
        throw new HttpError(401, 'invalid_credentials', '用户名或密码错误。');
      }
      // A CLI account change while verification was running invalidates this attempt.
      if (account(this.database)?.revision !== configured.revision) throw new HttpError(401, 'account_changed', '账户已更新，请重新登录。');
      this.failures = 0;
      this.blockedUntil = 0;
      const token = randomBytes(32).toString('hex');
      this.database.prepare('INSERT INTO sessions VALUES (?, ?, ?, ?)').run(tokenHash(token), configured.revision, Date.now(), Date.now());
      this.database.exec('DELETE FROM sessions WHERE token_hash NOT IN (SELECT token_hash FROM sessions ORDER BY created_at DESC, rowid DESC LIMIT 64)');
      return token;
    } finally {
      this.active -= 1;
    }
  }
}
