import assert from 'node:assert/strict';
import http from 'node:http';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { requestHealth, waitForHealth } from '../service-health.mjs';

async function endpoint(t, component, handler, host = '127.0.0.1') {
  const root = mkdtempSync(join(tmpdir(), 'aow-health-'));
  const server = http.createServer(handler);
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const socket = join(root, 'daemon.sock');
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    if (component === 'terminald') server.listen(socket, resolve);
    else server.listen(0, host, resolve);
  });
  t.after(async () => {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  });
  return { component, host, port: server.address()?.port, socket };
}

test('server health can reach an IPv6 loopback listener', async t => {
  let config;
  try {
    config = await endpoint(t, 'server', (_, response) => {
      response.end(JSON.stringify({ service: 'aow', ok: true, pid: 1234 }));
    }, '::1');
  } catch (error) {
    if (['EAFNOSUPPORT', 'EADDRNOTAVAIL'].includes(error.code)) {
      t.skip('IPv6 loopback is unavailable on this host');
      return;
    }
    throw error;
  }
  await waitForHealth(config, () => 1234, 1000);
});

for (const component of ['server', 'terminald']) {
  test(`${component} health waits for its own process and checks the expected endpoint`, async t => {
    let requests = 0;
    const config = await endpoint(t, component, (request, response) => {
      assert.equal(request.url, component === 'server' ? '/api/health' : '/v1/health');
      response.end(JSON.stringify({ service: component === 'server' ? 'aow' : 'aow-terminald',
        ok: true, pid: ++requests === 1 ? 999 : 1234 }));
    });
    await waitForHealth(config, () => 1234, 1000);
    assert.equal(requests, 2);
  });
}

test('health rejects an unrelated listener, malformed bodies and HTTP errors', async t => {
  for (const body of ['not json', '{}', JSON.stringify({ service: 'other', ok: true })]) {
    const config = await endpoint(t, 'server', (_, response) => response.end(body));
    await assert.rejects(requestHealth(config));
  }
  const config = await endpoint(t, 'server', (_, response) => { response.writeHead(503); response.end('{}'); });
  await assert.rejects(requestHealth(config), /503/);
});

test('server health follows the configured Base Path and normalizes its trailing slash', async t => {
  const config = await endpoint(t, 'server', (request, response) => {
    if (request.url !== '/tools/aow/api/health') response.writeHead(404);
    response.end(JSON.stringify({ service: 'aow', ok: true, pid: 1234 }));
  });
  await assert.rejects(requestHealth(config), /404/);
  for (const basePath of ['/tools/aow', '/tools/aow/']) {
    await waitForHealth({ ...config, basePath }, () => 1234, 1000);
  }
  for (const basePath of ['relative', '//', '/tools//', '/tools/../aow', '/tools?aow']) {
    await assert.rejects(requestHealth({ ...config, basePath }), /invalid AOW_BASE_PATH/);
  }
  const root = await endpoint(t, 'server', (request, response) => {
    assert.equal(request.url, '/api/health');
    response.end('{"service":"aow","ok":true}');
  });
  await requestHealth({ ...root, basePath: '/' });
});

test('health requires the PID advertised by a new release and detects restarts during the probe', async t => {
  const config = await endpoint(t, 'server', (_, response) => {
    response.end(JSON.stringify({ service: 'aow', ok: true, pid: 1234 }));
  });
  // Allow a real HTTP round trip under CI load before asserting PID ownership.
  // The separate unresponsive-endpoint test covers the request deadline.
  await assert.rejects(waitForHealth(config, () => 1235, 1000), /active launchd process/);
  let calls = 0;
  await assert.rejects(waitForHealth(config, () => ++calls % 2 ? 1234 : 1235, 1000), /active launchd process/);
  const old = await endpoint(t, 'server', (_, response) => response.end('{"service":"aow","ok":true}'));
  await assert.rejects(waitForHealth(old, () => 1234, 1000), /active launchd process/);
});

test('unresponsive health checks have a total deadline', async t => {
  const config = await endpoint(t, 'server', () => {});
  const started = Date.now();
  await assert.rejects(waitForHealth(config, () => 1234, 100), /timed out/);
  assert.ok(Date.now() - started < 1000);
});
