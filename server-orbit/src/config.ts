import { homedir } from 'node:os';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export interface Config {
  host: string;
  port: number;
  stateDir: string;
  frontendDir: string;
  frontendOrigin?: string;
  secureCookies: boolean;
  sessionMaxAge: number;
  sessionIdleTimeout: number;
}

function seconds(value: string | undefined, fallback: number): number {
  const result = Number(value ?? fallback);
  if (!Number.isSafeInteger(result) || result <= 0) throw new Error('Session timeouts must be positive integers.');
  return result;
}

export function configuration(env = process.env): Config {
  const port = Number(env.ORBIT_PORT ?? 8382);
  if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('Invalid ORBIT_PORT.');
  const frontendOrigin = env.ORBIT_FRONTEND_ORIGIN;
  if (frontendOrigin && new URL(frontendOrigin).origin !== frontendOrigin) {
    throw new Error('ORBIT_FRONTEND_ORIGIN must be an HTTP(S) origin without a trailing slash.');
  }
  if (frontendOrigin && !/^https?:\/\//.test(frontendOrigin)) throw new Error('Invalid frontend origin.');
  return {
    host: env.ORBIT_HOST ?? '127.0.0.1',
    port,
    stateDir: resolve(env.ORBIT_STATE_DIR ?? resolve(homedir(), '.orbit')),
    frontendDir: resolve(env.ORBIT_FRONTEND_DIR ?? fileURLToPath(new URL('../../frontend-orbit/dist', import.meta.url))),
    frontendOrigin,
    secureCookies: env.ORBIT_SECURE_COOKIES === '1',
    sessionMaxAge: seconds(env.ORBIT_SESSION_MAX_AGE_SECONDS, 12 * 60 * 60) * 1000,
    sessionIdleTimeout: seconds(env.ORBIT_SESSION_IDLE_TIMEOUT_SECONDS, 30 * 60) * 1000,
  };
}
