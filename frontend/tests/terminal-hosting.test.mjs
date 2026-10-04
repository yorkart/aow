import assert from 'node:assert/strict';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { createServer } from 'vite';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  const hosting = { id: 'hosting-one', task_id: 'review', task_name: '代码 Review', workspace_root: '/repo', phase: 'reviewing', max_inputs: 3, input_count: 0, run_id: 'run-one', error: null };
  const task = { id: 'review', revision: 3, name: '代码 Review', kind: 'manual', workspace_mode: 'dynamic', prompt: '检查本轮代码变更', project_id: 'project' };
  async function fixture(t, mobile = false) {
    const page = await browser.newPage({ viewport: { width: mobile ? 390 : 1100, height: 750 } });
    page.setDefaultTimeout(15000);
    const errors = [], starts = [], claims = [], inputs = [], requests = [];
    let hosted = false, socket, events, revision = 1;
    const notify = () => events?.send(JSON.stringify({ event: 'workspace', data: { boot_id: 'boot', revision: ++revision, projects: 0, terminals: revision, tasks: 0, repositories: {} } }));
    let currentHosting = mobile ? { ...hosting, phase: 'failed', error: 'Agent 异常退出' } : hosting;
    const currentTab = () => ({ id: 'tab', name: 'Agent', workspace_root: '/repo', revision,
      layout: { type: 'pane', pane_id: 'pane' }, panes: [{ id: 'pane', name: 'Agent', cwd: '/repo', shell: '/bin/sh', agent_id: 'codex', status: 'running', rows: 24, cols: 80, hosting: hosted ? currentHosting : undefined }] });
    page.on('pageerror', error => errors.push(error.message));
    t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
    await page.route('**/api/**', async route => {
      const url = new URL(route.request().url()); requests.push(url.pathname);
      if (url.pathname === '/api/terminals') return route.fulfill({ json: { tabs: [currentTab()] } });
      if (url.pathname === '/api/terminals/agents') return route.fulfill({ json: { agents: { pane: 'codex' }, titles: { pane: '实现登录功能' }, processes: {} } });
      if (url.pathname === '/api/aow/projects') return route.fulfill({ json: [{ id: 'project', worktrees: [{ id: 'worktree', path: '/repo' }] }] });
      if (url.pathname === '/api/aow/automations') {
        assert.equal(url.searchParams.get('project_id'), 'project');
        return route.fulfill({ json: [task, { ...task, id: 'schedule', kind: 'scheduled', name: '定时任务' }, ...['existing', 'new_worktree', 'temporary'].map(mode => ({ ...task, id: mode, workspace_mode: mode, name: `非动态任务 ${mode}` }))] });
      }
      if (url.pathname === '/api/aow/automations/review') return route.fulfill({ json: task });
      if (url.pathname === '/api/aow/automations/review/runs/run-one') return route.fulfill({ json: { id: 'run-one', task_id: 'review', status: 'failed' } });
      if (url.pathname.endsWith('/hosting')) {
        starts.push(route.request().postDataJSON()); hosted = true; currentHosting = { ...currentHosting, max_inputs: starts.at(-1).max_inputs };
        socket?.send(JSON.stringify({ type: 'error', code: 'attachment_superseded', message: 'controller changed' }));
        notify();
        return route.fulfill({ json: currentTab() });
      }
      return route.fulfill({ status: 404, json: {} });
    });
    await page.routeWebSocket('**/api/terminals/**/ws?**', current => {
      socket = current;
      current.onMessage(message => {
        if (typeof message !== 'string') { inputs.push(message.toString()); return; }
        const control = JSON.parse(message);
        if (control.type === 'write') inputs.push(control.data);
        if (control.type === 'claim') {
          claims.push(control.force);
          if (control.force) { hosted = false; notify(); if (!mobile) void page.evaluate(() => window.hostingPreview.update()); }
          current.send(JSON.stringify({ type: 'control', state: hosted ? 'observing' : 'claimed' }));
          current.send(JSON.stringify({ type: 'stream', epoch: 'epoch', offset: 0, reset: true, replay_bytes: 0, restore_cols: 80, restore_rows: 24, restore: '\x1b[2J\x1b[HAgent output\r\n> \x1b[?2004h' }));
        }
      });
    });
    await page.routeWebSocket('**/api/events/ws', current => { events = current; notify(); });
    await page.goto(`${base}/tests/terminal-hosting-preview.html${mobile ? '?mobile' : ''}`);
    await page.getByRole('button', { name: 'Autopilot', exact: true }).waitFor();
    return { page, starts, claims, inputs, requests, updateHosting: async patch => {
      currentHosting = { ...currentHosting, ...patch }; notify();
      if (!mobile) await page.evaluate(value => window.hostingPreview.update(value), currentHosting);
    } };
  }
  await test('selecting a manual task hosts the pane, displays state beside takeover, and blocks input', async t => {
    const { page, starts, claims, inputs } = await fixture(t);
    await page.getByRole('button', { name: 'Autopilot', exact: true }).click();
    const menu = page.getByRole('dialog', { name: '选择托管任务' });
    await menu.getByRole('button', { name: /代码 Review/ }).waitFor();
    assert.equal(await menu.getByText('定时任务').count(), 0);
    assert.equal(await menu.getByRole('button', { name: /非动态任务/ }).count(), 0);
    await menu.getByLabel('搜索手动任务').fill('无匹配');
    await menu.getByText('没有匹配的任务').waitFor();
    await menu.getByLabel('搜索手动任务').fill('Review');
    const limit = menu.getByLabel('最多自动输入次数');
    assert.equal(await limit.inputValue(), '3');
    for (const invalid of ['0', '1.5', '']) {
      await limit.fill(invalid);
      assert.equal(await menu.getByRole('button', { name: /代码 Review/ }).isDisabled(), true);
    }
    await limit.fill('3');
    await menu.getByRole('button', { name: /代码 Review/ }).click();
    await page.locator('.terminal-connection.hosting').getByText(/托管中 · 代码 Review/).waitFor();
    assert.deepEqual(starts, [{ task_id: 'review', revision: 3, max_inputs: 3 }]);
    assert.equal(await page.locator('.terminal-pane-header').getByText(/托管中/).count(), 0);
    assert.equal(await page.getByRole('button', { name: 'Autopilot', exact: true }).isDisabled(), true);
    await page.locator('.xterm-helper-textarea').focus(); await page.keyboard.type('do not send');
    assert.deepEqual(inputs, []);
    await page.locator('.terminal-connection').getByRole('button', { name: '接管', exact: true }).click();
    await page.waitForFunction(() => !document.querySelector('.terminal-connection.hosting'));
    assert.equal(claims.includes(true), true);
  });
  await test('failure warning opens the exact automation run tab and remains usable at narrow widths', async t => {
    const { page, requests } = await fixture(t);
    await page.evaluate(value => window.hostingPreview.update(value), { ...hosting, phase: 'failed', error: 'Agent 异常退出' });
    const warning = page.locator('.terminal-connection .terminal-hosting-details');
    await warning.getByText('Agent 异常退出').waitFor();
    const link = warning.getByRole('link', { name: '查看本次执行' });
    await link.waitFor();
    assert.match(await link.getAttribute('href'), /automation\/review\/runs\/run-one\?workspace=worktree/);
    if (process.env.AOW_HOSTING_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.AOW_HOSTING_SCREENSHOT_DIR}/aow-hosting-desktop.png` });
    await page.setViewportSize({ width: 390, height: 760 });
    assert.equal(await link.isVisible(), true);
    if (process.env.AOW_HOSTING_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.AOW_HOSTING_SCREENSHOT_DIR}/aow-hosting-narrow.png` });
    await link.click();
    await page.getByTestId('opened-run').getByText('run-one').waitFor();
    assert.ok(requests.includes('/api/aow/automations/review/runs/run-one'));
  });
  await test('takeover restores keyboard input after reopening an already hosted pane', async t => {
    const { page, inputs, claims } = await fixture(t);
    await page.getByRole('button', { name: 'Autopilot', exact: true }).click();
    await page.getByRole('dialog', { name: '选择托管任务' }).getByRole('button', { name: /代码 Review/ }).click();
    await page.locator('.terminal-connection.hosting').waitFor();
    await page.reload();
    const status = page.locator('.terminal-connection');
    await status.getByText(/托管中 · 代码 Review/).waitFor();
    await page.locator('.xterm-helper-textarea').focus();
    await page.keyboard.type('do not send');
    await page.keyboard.press('Enter');
    assert.deepEqual(inputs, []);
    await status.getByRole('button', { name: '接管', exact: true }).click();
    await page.locator('.terminal-connection.connected:not(.hosting)').waitFor();
    await page.locator('.xterm-helper-textarea').focus();
    await page.keyboard.type('hello');
    await page.keyboard.press('Enter');
    for (let attempt = 0; attempt < 20 && inputs.join('') !== 'hello\r'; attempt++) await delay(50);
    assert.equal(inputs.join(''), 'hello\r');
    assert.ok(claims.includes(true));
  });
  await test('mobile shows hosting failure beside takeover and disables the composer until takeover', async t => {
    const { page, claims } = await fixture(t, true);
    await page.getByRole('button', { name: 'Autopilot', exact: true }).click();
    await page.getByRole('dialog', { name: '选择托管任务' }).getByRole('button', { name: /代码 Review/ }).click();
    const status = page.locator('.mobile-terminal-hosting');
    await status.getByText('Agent 异常退出').waitFor();
    await status.getByRole('link', { name: '查看本次执行' }).waitFor();
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).isDisabled(), true);
    if (process.env.AOW_HOSTING_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.AOW_HOSTING_SCREENSHOT_DIR}/aow-hosting-mobile.png` });
    await status.getByRole('button', { name: '接管', exact: true }).click();
    await page.waitForFunction(() => !document.querySelector('.mobile-terminal-hosting'));
    assert.ok(claims.includes(true));
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).isDisabled(), false);
  });
  for (const mobile of [false, true]) {
    await test(`${mobile ? 'mobile' : 'desktop'} preserves stopped outcomes, counts and observer controls`, async t => {
      const { page, starts, inputs, claims, updateHosting } = await fixture(t, mobile);
      await page.getByRole('button', { name: 'Autopilot', exact: true }).click();
      const menu = page.getByRole('dialog', { name: '选择托管任务' });
      await menu.getByLabel('最多自动输入次数').fill('2');
      await menu.getByRole('button', { name: /代码 Review/ }).click();
      assert.deepEqual(starts, [{ task_id: 'review', revision: 3, max_inputs: 2 }]);
      const status = page.locator(mobile ? '.mobile-terminal-hosting' : '.terminal-connection.hosting');
      await status.getByText(/已输入 0\/2 次/).waitFor();
      for (const [phase, label] of [['completed', '托管已结束'], ['limit_reached', '托管已停止']]) {
        await updateHosting({ phase, input_count: 2, error: null });
        await status.getByText(new RegExp(`${label} · 代码 Review .*已输入 2/2 次`)).waitFor();
        await status.getByRole('link', { name: '查看本次执行' }).waitFor();
        if (mobile) assert.equal(await page.getByRole('textbox', { name: '终端命令' }).isDisabled(), true);
        else { await page.locator('.xterm-helper-textarea').focus(); await page.keyboard.type('do not send'); }
        assert.deepEqual(inputs, []);
        if (process.env.AOW_HOSTING_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.AOW_HOSTING_SCREENSHOT_DIR}/aow-hosting-${mobile ? 'mobile' : 'desktop'}-${phase}.png` });
      }
      if (mobile) {
        await page.goto(`${base}/tests/terminal-hosting-preview.html?mobile`);
        await status.getByText(/托管已停止 .*已输入 2\/2 次/).waitFor();
        assert.equal(await page.getByRole('textbox', { name: '终端命令' }).isDisabled(), true);
      }
      await status.getByRole('button', { name: '接管', exact: true }).click();
      await status.waitFor({ state: 'detached' });
      assert.ok(claims.includes(true));
    });
  }
} finally { await browser?.close(); await server.close(); }
