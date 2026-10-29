import { randomUUID } from 'node:crypto';
import { readFile, realpath, stat } from 'node:fs/promises';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { extname, resolve, sep } from 'node:path';
import type { DatabaseSync } from 'node:sqlite';
import { Auth } from './auth.js';
import type { Config } from './config.js';
import { orgState, transaction } from './database.js';
import { HttpError } from './errors.js';
import { body, json } from './requests.js';
import { Skills } from './skills/service.js';
import { SkillRoutes } from './skills/routes.js';
import { Connectors } from './connectors/service.js';
import { ConnectorRoutes } from './connectors/routes.js';
import type { AdapterOptions } from './connectors/adapters.js';

function checkOrigin(request: IncomingMessage, config: Config): void {
  const origin = request.headers.origin;
  if (!origin) {
    if (request.headers['sec-fetch-site'] === 'cross-site') throw new HttpError(403, 'invalid_origin', '请求来源无效。');
    return;
  }
  try {
    const url = new URL(origin);
    if (/^https?:$/.test(url.protocol) && url.origin === origin &&
      (url.host.toLowerCase() === request.headers.host?.toLowerCase() || origin === config.frontendOrigin)) return;
  } catch { /* Invalid origins are rejected below. */ }
  throw new HttpError(403, 'invalid_origin', '请求来源无效。');
}

const mime: Record<string, string> = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.png': 'image/png',
  '.ico': 'image/x-icon', '.woff2': 'font/woff2',
};

async function frontend(request: IncomingMessage, response: ServerResponse, pathname: string, config: Config): Promise<void> {
  if (request.method !== 'GET' && request.method !== 'HEAD') throw new HttpError(405, 'method_not_allowed', '请求方法不支持。');
  const root = await realpath(config.frontendDir).catch(() => undefined);
  if (!root) throw new HttpError(503, 'frontend_not_built', '请先运行 Orbit 构建脚本。');
  let decoded: string;
  try { decoded = decodeURIComponent(pathname); } catch { throw new HttpError(400, 'invalid_path', '路径无效。'); }
  if (decoded.includes('\0') || decoded.includes('\\')) throw new HttpError(400, 'invalid_path', '路径无效。');
  let file = resolve(root, `.${decoded === '/' ? '/index.html' : decoded}`);
  if (!file.startsWith(root + sep)) throw new HttpError(404, 'not_found', '内容不存在。');
  if (!(await stat(file).catch(() => undefined))?.isFile()) {
    if (extname(decoded)) throw new HttpError(404, 'not_found', '内容不存在。');
    file = resolve(root, 'index.html');
  }
  const canonical = await realpath(file).catch(() => undefined);
  if (!canonical?.startsWith(root + sep)) throw new HttpError(404, 'not_found', '内容不存在。');
  const content = await readFile(canonical);
  response.writeHead(200, {
    'Content-Type': mime[extname(file)] ?? 'application/octet-stream',
    'Content-Length': content.length,
    'Cache-Control': pathname.startsWith('/assets/') ? 'public, max-age=31536000, immutable' : 'no-cache',
    'Content-Security-Policy': "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
  });
  response.end(request.method === 'HEAD' ? undefined : content);
}

export function createOrbitServer(config: Config, database: DatabaseSync, connectorOptions?: AdapterOptions) {
  const auth = new Auth(database, config);
  const skillRoutes = new SkillRoutes(new Skills(database, config.stateDir));
  const connectors = new Connectors(database, config.stateDir, connectorOptions);
  const connectorRoutes = new ConnectorRoutes(connectors);
  const server = createServer({ requestTimeout: 15_000, headersTimeout: 10_000 }, (request, response) => {
    response.setHeader('X-Content-Type-Options', 'nosniff');
    response.setHeader('Referrer-Policy', 'same-origin');
    const handle = async () => {
      const pathname = new URL(request.url ?? '/', 'http://orbit.local').pathname;
      if (pathname !== '/api' && !pathname.startsWith('/api/')) return frontend(request, response, pathname, config);
      response.setHeader('Cache-Control', 'no-store');
      checkOrigin(request, config);
      if (request.headers.origin && request.headers.origin === config.frontendOrigin) {
        response.setHeader('Access-Control-Allow-Origin', request.headers.origin);
        response.setHeader('Access-Control-Allow-Credentials', 'true');
        response.setHeader('Vary', 'Origin');
      }
      if (request.method === 'OPTIONS') {
        response.writeHead(204, {
          'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, OPTIONS',
          'Access-Control-Allow-Headers': 'Content-Type',
        });
        response.end();
        return;
      }
      if (pathname === '/api/health' && request.method === 'GET') return json(response, 200, { status: 'ok' });
      if (pathname === '/api/auth/status' && request.method === 'GET') return json(response, 200, auth.status(request));
      if (pathname === '/api/auth/login' && request.method === 'POST') {
        const token = await auth.login(await body(request));
        auth.revoke(request);
        response.setHeader('Set-Cookie', auth.cookie(token));
        return json(response, 200, { configured: true, authenticated: true });
      }
      if (pathname === '/api/auth/logout' && request.method === 'POST') {
        auth.revoke(request);
        response.setHeader('Set-Cookie', auth.cookie(''));
        response.writeHead(204);
        response.end();
        return;
      }
      if (!auth.authenticated(request, true)) throw new HttpError(401, 'unauthorized', '请重新登录。');
      if (pathname === '/api/orgs' && request.method === 'GET') return json(response, 200, orgState(database));
      if (pathname === '/api/orgs' && request.method === 'POST') {
        const { name } = await body(request);
        if (typeof name !== 'string' || !name.trim() || [...name.trim()].length > 80 || /\p{Cc}/u.test(name)) {
          throw new HttpError(422, 'invalid_org_name', '组织名称须为 1–80 个字符，且不能包含控制字符。');
        }
        const state = transaction(database, () => {
          const id = randomUUID();
          database.prepare('INSERT INTO orgs (id, name, created_at) VALUES (?, ?, ?)').run(id, name.trim(), new Date().toISOString());
          database.prepare('UPDATE account SET active_org_id = ? WHERE id = 1').run(id);
          return orgState(database);
        });
        return json(response, 201, state);
      }
      if (pathname === '/api/orgs/active' && request.method === 'PUT') {
        const { id } = await body(request);
        if (typeof id !== 'string' || !database.prepare('SELECT id FROM orgs WHERE id = ?').get(id)) {
          throw new HttpError(404, 'org_not_found', '组织不存在，请刷新后重试。');
        }
        database.prepare('UPDATE account SET active_org_id = ? WHERE id = 1').run(id);
        return json(response, 200, orgState(database));
      }
      if (await skillRoutes.handle(request, response, new URL(request.url ?? '/', 'http://orbit.local'))) return;
      if (await connectorRoutes.handle(request, response, new URL(request.url ?? '/', 'http://orbit.local'))) return;
      throw new HttpError(404, 'not_found', '接口不存在。');
    };
    void handle().catch((error: unknown) => {
      if (response.destroyed || response.writableEnded) return;
      const failure = error instanceof HttpError ? error : new HttpError(500, 'internal_error', '服务暂时不可用，请重试。');
      if (!(error instanceof HttpError)) console.error('Orbit request failed:', error instanceof Error ? error.message : 'Unknown error');
      if (failure.retryAfter) response.setHeader('Retry-After', failure.retryAfter);
      json(response, failure.status, { code: failure.code, message: failure.message });
    });
  });
  server.on('close', () => { void connectors.close().catch(() => {}); });
  return server;
}
