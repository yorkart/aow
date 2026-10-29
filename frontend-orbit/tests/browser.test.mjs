import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { existsSync } from 'node:fs';
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { chromium } from 'playwright';
import { setAccount } from '../../server-orbit/dist/auth.js';
import { openDatabase } from '../../server-orbit/dist/database.js';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const executablePath = process.env.ORBIT_TEST_BROWSER || (existsSync('/usr/bin/chromium') ? '/usr/bin/chromium' : undefined);

test('login → required onboarding → real orgs → empty icon-only workspace → reload, logout and restart', { timeout: 60_000 }, async t => {
  const directory = await mkdtemp(join(tmpdir(), 'orbit-browser-'));
  const database = openDatabase(directory);
  await setAccount(database, 'orbit-user', 'test-only-password');
  database.close();
  let server;
  let url;
  const start = async () => {
    server = spawn(process.execPath, ['server-orbit/dist/main.js'], {
      cwd: repository, stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, ORBIT_STATE_DIR: directory, ORBIT_PORT: '0', ORBIT_HOST: '127.0.0.1' },
    });
    url = await new Promise((resolveUrl, reject) => {
      let output = '';
      const timeout = setTimeout(() => reject(new Error('Orbit did not start.')), 10_000);
      server.stdout.on('data', chunk => {
        output += chunk;
        const match = output.match(/Orbit: (http:\/\/\S+)/);
        if (match) { clearTimeout(timeout); resolveUrl(match[1]); }
      });
      server.on('error', error => { clearTimeout(timeout); reject(error); });
      server.on('exit', code => { clearTimeout(timeout); reject(new Error(`Orbit exited with ${code}.`)); });
    });
  };
  const stop = async () => {
    if (server && server.exitCode === null && server.signalCode === null) {
      const exited = once(server, 'exit');
      server.kill('SIGTERM');
      await exited;
    }
  };
  t.after(async () => { await stop(); await rm(directory, { recursive: true, force: true }); });
  await start();
  const browser = await chromium.launch({ executablePath, headless: true, args: ['--no-sandbox'] });
  t.after(() => browser.close());
  const context = await browser.newContext({ locale: 'zh-CN', viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const screenshot = async name => {
    if (!process.env.ORBIT_TEST_ARTIFACT_DIR) return;
    await mkdir(process.env.ORBIT_TEST_ARTIFACT_DIR, { recursive: true });
    await page.screenshot({ path: join(process.env.ORBIT_TEST_ARTIFACT_DIR, `${name}.png`), fullPage: true });
  };
  await page.goto(url);
  await page.getByLabel('账号', { exact: true }).fill('orbit-user');
  await page.getByLabel('密码', { exact: true }).fill('wrong');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.getByRole('alert').waitFor();
  await page.getByLabel('密码', { exact: true }).fill('test-only-password');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.getByRole('heading', { name: '创建你的第一个组织' }).waitFor();
  await screenshot('onboarding');
  await page.reload();
  await page.getByRole('heading', { name: '创建你的第一个组织' }).waitFor();
  await page.getByLabel('组织名称').fill('York Studio');
  await page.getByRole('button', { name: '创建并进入工作空间' }).click();
  const switcher = () => page.getByRole('button', { name: /^切换组织，当前/ });
  await switcher().waitFor();
  assert.match(await switcher().getAttribute('aria-label'), /York Studio/);
  const rail = page.getByRole('navigation', { name: '功能导航' });
  assert.deepEqual(await rail.getByRole('button').evaluateAll(buttons => buttons.map(button => button.getAttribute('aria-label'))), ['Tasks', 'Projects', 'Automation', 'Agents', 'Skills', 'Connectors']);
  assert.deepEqual(await rail.getByRole('button').allTextContents(), ['', '', '', '', '', '']);
  for (const section of ['Automation', 'Tasks']) {
    await rail.getByRole('button', { name: section, exact: true }).click();
    assert.equal(await rail.getByRole('button', { name: section, exact: true }).getAttribute('aria-current'), 'page');
    assert.equal(await page.getByRole('main', { name: `${section} 工作区` }).innerHTML(), '');
    assert.equal(await page.getByLabel(`${section} 列表`, { exact: true }).innerHTML(), '');
  }
  await rail.getByRole('button', { name: 'Projects', exact: true }).click();
  await page.getByRole('heading', { name: '创建你的第一个项目' }).waitFor();
  await rail.getByRole('button', { name: 'Agents', exact: true }).click();
  await page.getByRole('heading', { name: '创建你的第一个 Agent' }).waitFor();
  await rail.getByRole('button', { name: 'Skills', exact: true }).click();
  await page.getByRole('heading', { name: 'Installed skills', exact: true }).waitFor();
  await page.getByRole('button', { name: 'Installed', exact: true }).waitFor();
  await rail.getByRole('button', { name: 'Connectors', exact: true }).click();
  await page.getByRole('heading', { name: '本地 Connector', exact: true }).waitFor();
  await page.getByRole('button', { name: '本地 Connector', exact: true }).waitFor();
  await rail.getByRole('button', { name: 'Tasks', exact: true }).click();
  const palette = await page.evaluate(() => ({ theme: document.documentElement.dataset.theme, surface: getComputedStyle(document.documentElement).getPropertyValue('--aow-surface').trim(), aowPreference: localStorage.getItem('aow.theme') }));
  assert.deepEqual(palette, { theme: 'aow-dark', surface: '#181a1f', aowPreference: null });
  await screenshot('workspace');
  await switcher().click();
  await page.getByRole('button', { name: '创建组织', exact: true }).click();
  await page.getByRole('dialog').waitFor();
  await page.getByLabel('组织名称').fill('第二个组织');
  await page.getByRole('button', { name: '创建组织', exact: true }).click();
  await page.getByRole('button', { name: '切换组织，当前 第二个组织' }).waitFor();
  await switcher().click();
  await page.getByRole('group', { name: '组织列表' }).getByRole('button', { name: 'York Studio' }).click();
  await page.getByRole('button', { name: '切换组织，当前 York Studio' }).waitFor();
  await page.reload();
  await page.getByRole('button', { name: '切换组织，当前 York Studio' }).waitFor();
  await page.setViewportSize({ width: 375, height: 812 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  await switcher().click();
  await page.getByRole('button', { name: '退出登录' }).click();
  await page.getByLabel('账号', { exact: true }).waitFor();
  await page.reload();
  await page.getByLabel('账号', { exact: true }).waitFor();
  await stop();
  await start();
  await page.goto(url);
  await page.getByLabel('账号', { exact: true }).fill('orbit-user');
  await page.getByLabel('密码', { exact: true }).fill('test-only-password');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.getByRole('button', { name: '切换组织，当前 York Studio' }).waitFor();
  await switcher().click();
  assert.equal(await page.getByRole('group', { name: '组织列表' }).getByRole('button').count(), 2);
  assert.deepEqual(errors, []);
});
