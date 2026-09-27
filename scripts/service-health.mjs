import { execFileSync } from 'node:child_process';
import { readFileSync, realpathSync } from 'node:fs';
import http from 'node:http';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

export function serverHealthPath(basePath = '') {
  // Match BasePath::parse in the server, including one optional trailing slash.
  const prefix = basePath.endsWith('/') ? basePath.slice(0, -1) : basePath;
  if (prefix === '') return '/api/health';
  if (prefix.length > 1024 || !/^\/[A-Za-z0-9._~-]+(?:\/[A-Za-z0-9._~-]+)*$/.test(prefix)
    || prefix.slice(1).split('/').some(segment => segment === '.' || segment === '..')) {
    throw new Error('invalid AOW_BASE_PATH for service health check');
  }
  return `${prefix}/api/health`;
}

export function requestHealth(config, timeout = 1000) {
  return new Promise((resolve, reject) => {
    const options = config.component === 'terminald'
      ? { socketPath: config.socket, path: '/v1/health' }
      : { hostname: config.host, port: config.port, path: serverHealthPath(config.basePath) };
    const request = http.get({ ...options, agent: false }, response => {
      response.setEncoding('utf8');
      let bytes = '';
      response.on('data', chunk => {
        bytes += chunk;
        if (bytes.length > 16384) request.destroy(new Error('health response is too large'));
      });
      response.on('error', reject);
      response.on('end', () => {
        try {
          if (response.statusCode !== 200) throw new Error(`health returned HTTP ${response.statusCode}`);
          const health = JSON.parse(bytes);
          const service = config.component === 'terminald' ? 'aow-terminald' : 'aow';
          if (health.service !== service || (config.component === 'server' && health.ok !== true)) {
            throw new Error('unexpected health response');
          }
          resolve(health);
        } catch (error) { reject(error); }
      });
    });
    const timer = setTimeout(() => request.destroy(new Error('health request timed out')), timeout);
    request.on('close', () => clearTimeout(timer));
    request.on('error', reject);
  });
}

export async function waitForHealth(config, runningPid, timeout = 10000) {
  if (!['server', 'terminald'].includes(config.component)) throw new Error('invalid service component');
  const deadline = Date.now() + timeout;
  let last;
  do {
    try {
      const pid = runningPid();
      if (!Number.isSafeInteger(pid) || pid <= 1) throw new Error('launchd job is not running');
      const health = await requestHealth(config, Math.max(1, Math.min(1000, deadline - Date.now())));
      if (health.pid !== pid || runningPid() !== pid) {
        throw new Error('health endpoint does not belong to the active launchd process');
      }
      return;
    } catch (error) { last = error; }
    await delay(Math.min(100, Math.max(0, deadline - Date.now())));
  } while (Date.now() < deadline);
  throw new Error(`aow-${config.component} did not become healthy: ${last?.message}`);
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const config = JSON.parse(readFileSync(process.argv[2], 'utf8'));
    await waitForHealth(config, () => {
      const status = execFileSync('launchctl', ['print', process.argv[3]], {
        encoding: 'utf8', timeout: 1000, stdio: ['ignore', 'pipe', 'ignore'],
      });
      if (!/^\s*state = running\s*$/m.test(status)) return null;
      return Number(status.match(/^\s*pid = (\d+)\s*$/m)?.[1]);
    });
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
