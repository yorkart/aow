import { loadModule } from './helpers/load-module.mjs';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';

const { serverEnvironmentFields, readServerEnvironment: read, writeServerEnvironment: write, validateServerEnvironment: validate } = await loadModule(new URL('../src/features/settings/serverEnvironment.ts', import.meta.url));

test('the table covers the installer allowlist and proxies, excluding installer-owned values', async () => {
  const installer = await readFile(new URL('../../scripts/launchd-service.mjs', import.meta.url), 'utf8');
  const keys = name => [...installer.match(new RegExp(`const ${name} = new Set\\(\\[([\\s\\S]*?)\\]\\)`))[1].matchAll(/'([^']+)'/g)].map(match => match[1]);
  const fixed = ['HOME', 'USER', 'LOGNAME', 'AOW_RUNTIME_ROOT', 'AOW_LOG_MODE'];
  assert.deepEqual(serverEnvironmentFields.map(field => field.key).sort(), [...keys('daemonKeys').filter(key => !fixed.includes(key)), ...keys('proxyKeys')].sort());
});

test('unedited files round trip exactly, and defaults are never inserted', () => {
  for (const content of ['', '# comment\r\n\r\nAOW_SERVER_PORT = "8282" \r\nCUSTOM=$HOME=a=b\r\n', 'HTTP_PROXY=\nHTTP_PROXY=http://localhost:7890\n# last']) {
    for (const platform of ['macos', 'linux']) assert.equal(write(content, read(content, platform), platform), content);
  }
  assert.deepEqual(read('', 'linux'), {});
});

test('edits preserve comments, unmanaged configuration, quotes and line endings; clearing removes every duplicate', () => {
  const content = '# comment\r\n AOW_SERVER_PORT = "8281" \r\nCUSTOM=$HOME=a=b\r\n\r\nAOW_SERVER_PORT=8282\r\nHTTPS_PROXY=\'http://localhost:7890\'\r\n';
  const values = read(content, 'macos');
  assert.equal(values.AOW_SERVER_PORT, '8282');
  assert.equal(values.HTTPS_PROXY, 'http://localhost:7890');
  assert.equal(write(content, { ...values, AOW_SERVER_PORT: '', HTTPS_PROXY: 'http://localhost:7891' }, 'macos'),
    '# comment\r\nCUSTOM=$HOME=a=b\r\n\r\nHTTPS_PROXY=\'http://localhost:7891\'\r\n');
  assert.equal(write('AOW_SERVER_PORT=8281\nAOW_SERVER_PORT=8282', { AOW_SERVER_PORT: '9000', NO_PROXY: 'localhost,::1' }, 'macos'),
    'AOW_SERVER_PORT=9000\nNO_PROXY=localhost,::1\n');
});

test('proxy case variants are independent and literal values round trip without shell expansion', () => {
  for (const platform of ['macos', 'linux']) {
    const values = { HTTP_PROXY: 'http://user:p=a$word@localhost:7890', http_proxy: 'http://localhost:7891', PATH: '/with space/bin:/usr/bin', SHELL: "/path/with'quote/and\\slash" };
    assert.deepEqual(read(write('', values, platform), platform), values);
    const content = 'HTTP_PROXY=http://localhost:7890\nhttp_proxy=http://localhost:7891\n';
    assert.equal(write(content, { HTTP_PROXY: '', http_proxy: 'http://localhost:7891' }, platform), 'http_proxy=http://localhost:7891\n');
  }
});

test('Linux multiline entries cannot expose embedded assignments for editing', () => {
  const content = 'CUSTOM="first\nAOW_SERVER_PORT=1234\nlast"\nPATH=/usr/local/bin:\\\n/usr/bin\nAOW_SERVER_PORT=8282\n';
  const values = read(content, 'linux');
  assert.deepEqual(values, { PATH: '/usr/local/bin:/usr/bin', AOW_SERVER_PORT: '8282' });
  assert.equal(write(content, { ...values, AOW_SERVER_PORT: '9000' }, 'linux'), content.replace('AOW_SERVER_PORT=8282', 'AOW_SERVER_PORT=9000'));
  assert.equal(write(content, { ...values, PATH: '/bin' }, 'linux'), content.replace('PATH=/usr/local/bin:\\\n/usr/bin', 'PATH=/bin'));
});

test('session limits reject invalid seconds and allow the full unsigned 64-bit range', () => {
  for (const key of ['AOW_AUTH_SESSION_MAX_AGE_SECONDS', 'AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS']) {
    for (const value of ['0', '-1', '0.5', '12h', '18446744073709551616']) {
      assert.ok(validate({ [key]: value })[key]);
      assert.throws(() => write('', { [key]: value }, 'macos'), new RegExp(key));
    }
    for (const value of ['', '1', '43200', '18446744073709551615']) assert.deepEqual(validate({ [key]: value }), {});
  }
  assert.ok(validate({ AOW_SERVER_PORT: '65536' }).AOW_SERVER_PORT);
  assert.ok(validate({ AOW_AUTH_SECURE_COOKIE: 'yes' }).AOW_AUTH_SECURE_COOKIE);
  assert.throws(() => write('', { HTTP_PROXY: 'http://localhost\nINJECTED=value' }, 'macos'), /HTTP_PROXY/);
});
