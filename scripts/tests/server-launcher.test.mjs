import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const launcher = fileURLToPath(new URL('../../packaging/bin/aow-server', import.meta.url));
const generator = fileURLToPath(new URL('../launchd-service.mjs', import.meta.url));
const proxyKeys = ['HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'NO_PROXY', 'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy'];
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;

function fixture(t) {
  const home = realpathSync(mkdtempSync(join(tmpdir(), 'aow-server-host-')));
  t.after(() => rmSync(home, { recursive: true, force: true }));
  const runtime = join(home, '.local/lib/aow');
  const release = join(runtime, 'releases/test');
  for (const path of ['bin', 'frontend/dist']) mkdirSync(join(release, path), { recursive: true });
  mkdirSync(join(runtime, 'active'));
  symlinkSync('../releases/test', join(runtime, 'active/server'));
  writeFileSync(join(release, 'frontend/dist/index.html'), '<html></html>');
  // Capture the arguments passed to the binary by the real installed launcher.
  writeFileSync(join(release, 'bin/aow-server'), '#!/bin/sh\nprintf "%s\\n" "$@"\n', { mode: 0o700 });
  mkdirSync(join(home, '.config/aow'), { recursive: true });
  const env = { ...process.env, HOME: home, AOW_RUNTIME_ROOT: runtime };
  for (const key of Object.keys(env)) {
    if (key.startsWith('AOW_') && key !== 'AOW_RUNTIME_ROOT') delete env[key];
    if (proxyKeys.includes(key)) delete env[key];
  }
  function run(command, args, extra = {}) {
    const result = spawnSync(command, args, { env: { ...env, ...extra }, encoding: 'utf8', timeout: 5000 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stdout + result.stderr);
    return result.stdout;
  }
  function host(extra = {}) {
    const args = run('/bin/sh', [launcher], extra).trim().split('\n');
    assert.ok(args.includes('--host'));
    return args[args.indexOf('--host') + 1];
  }
  return { home, runtime, release, run, host };
}

test('installed server listens on loopback unless a host is explicitly configured', t => {
  const f = fixture(t);
  assert.equal(f.host(), '127.0.0.1');
  assert.equal(f.host({ AOW_SERVER_HOST: '' }), '127.0.0.1');
  for (const host of ['0.0.0.0', '192.0.2.10', '::1', '::', '[::1]', '[::]']) {
    assert.equal(f.host({ AOW_SERVER_HOST: host }), host);
  }
});

test('launchd preserves host opt-ins and probes the same interface as the installed server', t => {
  const f = fixture(t);
  const plist = join(f.home, 'server.plist');
  const config = join(f.home, '.config/aow/server.env');
  const cases = [
    { content: '', extra: {}, host: '127.0.0.1', health: '127.0.0.1' },
    { content: 'AOW_SERVER_HOST=\n', extra: {}, host: '127.0.0.1', health: '127.0.0.1' },
    { content: 'AOW_SERVER_HOST=0.0.0.0\n', extra: {}, host: '0.0.0.0', health: '127.0.0.1' },
    { content: 'AOW_SERVER_HOST=192.0.2.10\n', extra: {}, host: '192.0.2.10', health: '192.0.2.10' },
    { content: 'AOW_SERVER_HOST=::\n', extra: {}, host: '::', health: '::1' },
    { content: 'AOW_SERVER_HOST=[::]\n', extra: {}, host: '[::]', health: '::1' },
    { content: 'AOW_SERVER_HOST=::1\n', extra: {}, host: '::1', health: '::1' },
    { content: 'AOW_SERVER_HOST=[::1]\n', extra: {}, host: '[::1]', health: '::1' },
    { content: 'AOW_SERVER_HOST=[2001:db8::10]\n', extra: {}, host: '[2001:db8::10]', health: '2001:db8::10' },
    { content: 'AOW_SERVER_HOST=0.0.0.0\n', extra: { AOW_SERVER_HOST: '127.0.0.1' }, host: '127.0.0.1', health: '127.0.0.1' },
  ];
  for (const { content, extra, host, health } of cases) {
    writeFileSync(config, content);
    f.run(process.execPath, [generator, 'server', f.runtime, plist], extra);
    const xml = readFileSync(plist, 'utf8');
    assert.ok(xml.includes(join(f.runtime, 'bin/aow-server')));
    const configured = xml.match(/<key>AOW_SERVER_HOST<\/key><string>([^<]*)<\/string>/)?.[1];
    assert.equal(f.host(configured === undefined ? {} : { AOW_SERVER_HOST: configured }), host);
    assert.equal(JSON.parse(readFileSync(join(f.home, 'health.json'), 'utf8')).host, health);
  }
});

test('server launcher reloads literal proxy settings on every start without changing service options', t => {
  const f = fixture(t);
  const config = join(f.home, '.config/aow/server.env');
  const marker = join(f.home, 'must-not-execute');
  const capture = `console.log(JSON.stringify(Object.fromEntries(${JSON.stringify([...proxyKeys, 'AOW_SERVER_HOST'])}.map(key => [key, process.env[key] ?? null]))))`;
  writeFileSync(join(f.release, 'bin/aow-server'), `#!/bin/sh\nexec ${quote(process.execPath)} -e ${quote(capture)}\n`, { mode: 0o700 });
  const read = (extra = {}) => JSON.parse(f.run('/bin/sh', [launcher], extra));
  assert.equal(read({ HTTPS_PROXY: 'http://inherited:8000' }).HTTPS_PROXY, 'http://inherited:8000');
  const expected = Object.fromEntries(proxyKeys.map(key => [key, key.toLowerCase() === 'no_proxy' ? 'localhost,127.0.0.1,::1' : 'http://127.0.0.1:7890']));
  expected.HTTP_PROXY = `http://proxy/$(touch ${marker})/\`touch ${marker}\`/$HOME`;
  writeFileSync(config, '# preserved comment\r\nAOW_SERVER_HOST=0.0.0.0\r\n' + Object.entries(expected).map(([key, value]) => ` \t${key} = "${value}" \t`).join('\r\n'));
  assert.deepEqual(read({ HTTPS_PROXY: 'http://inherited:8000', AOW_SERVER_HOST: '127.0.0.1' }), { ...expected, AOW_SERVER_HOST: '127.0.0.1' });
  assert.equal(existsSync(marker), false);
  writeFileSync(config, "HTTPS_PROXY='http://changed:8001'\nHTTP_PROXY=\n");
  const changed = read();
  assert.equal(changed.HTTPS_PROXY, 'http://changed:8001');
  assert.equal(changed.HTTP_PROXY, '');
  assert.equal(changed.NO_PROXY, null);
  rmSync(config);
  const removed = read();
  assert.ok(proxyKeys.every(key => removed[key] === null));
});

for (const mode of ['launchagent', 'launchdaemon']) {
  test(`${mode} configuration stays unchanged when proxy settings change`, {
    skip: mode === 'launchdaemon' && process.getuid() === 0,
  }, t => {
    const f = fixture(t);
    const plist = join(f.home, 'server.plist');
    const generate = () => {
      f.run(process.execPath, [generator, 'server', f.runtime, plist, process.execPath, mode]);
      return readFileSync(plist, 'utf8');
    };
    const baseline = generate();
    writeFileSync(join(f.home, '.config/aow/server.env'), proxyKeys.map(key => `${key}=http://127.0.0.1:7890`).join('\n'));
    assert.equal(generate(), baseline);
  });
}
