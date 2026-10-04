import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });

  async function fixture(t, { bot = false, wechat = false, selected = false, unavailable = false } = {}) {
    const page = await browser.newPage({ viewport: { width: 1400, height: 1000 } });
    t.after(() => page.close());
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/api/aow/projects/*/branches', route => route.fulfill({ json: { branches: ['main', 'origin/main', 'origin/release'], default_branch: 'origin/main' } }));
    await page.route('**/api/aow/notification-settings', route => route.fulfill({
      status: unavailable ? 503 : 200,
      json: unavailable ? { message: 'unavailable' } : { im: { providers: [...(bot ? [{ provider: 'feishu', app_id: 'test', secret_configured: true }] : []), ...(wechat ? [{ provider: 'wechat', account_id: 'bot', user_id: 'owner' }] : [])] }, notifications: { agent_task_completed: { enabled: false, channels: [] } } },
    }));
    await page.route('**/api/aow/automations/12345678', route => route.fulfill({ json: route.request().postDataJSON() }));
    await page.goto(`${base}/tests/automation-notifications-preview.html${selected ? '?feishu' : ''}`);
    await page.getByLabel('失败提醒').waitFor();
    await page.getByText('正在检查 IM Bot 配置…', { exact: true }).waitFor({ state: 'hidden' });
    return { page, errors };
  }

  for (const mode of ['new_worktree', 'temporary']) {
    await test(`shared workspace selector saves Automation ${mode} configuration`, async t => {
      const { page, errors } = await fixture(t);
      const choices = page.getByRole('group', { name: '工作区方式', exact: true });
      assert.equal(await choices.getByRole('button').count(), 3);
      await choices.getByRole('button', { name: mode === 'temporary' ? '动态工作区' : '新建 Worktree', exact: true }).click();
      if (mode === 'new_worktree') {
        const branch = page.getByRole('combobox', { name: '分支来自', exact: true });
        await branch.getByRole('option', { name: 'origin/main（默认）', exact: true }).waitFor({ state: 'attached' });
        await branch.selectOption('origin/release');
      } else {
        assert.equal(await page.getByRole('radio', { name: '动态指定', exact: true }).isDisabled(), true);
        assert.equal(await page.getByRole('radio', { name: '临时工作区', exact: true }).isChecked(), true);
        assert.equal(await page.getByRole('combobox', { name: /已有 Worktree|分支来自/ }).count(), 0);
        await page.getByText('任务结束后自动删除临时工作区；该任务的 Agent 会话不会出现在项目的普通会话列表中。', { exact: true }).waitFor();
      }
      await page.getByRole('button', { name: '保存更改' }).click();
      const saved = JSON.parse(await page.getByTestId('saved').textContent());
      assert.equal(saved.workspace_mode, mode);
      assert.equal(saved.workspace_path, '');
      assert.equal(saved.base_branch, mode === 'new_worktree' ? 'origin/release' : '');
      assert.deepEqual(errors, []);
    });
  }

  await test('configured bot can be selected even when interactive notifications are disabled', async t => {
    const { page, errors } = await fixture(t, { bot: true });
    const select = page.getByLabel('失败提醒');
    assert.equal(await select.inputValue(), '');
    assert.equal(await select.locator('option[value="feishu"]').evaluate(option => option.disabled), false);
    await select.selectOption('feishu');
    await page.getByRole('button', { name: '保存更改' }).click();
    const saved = JSON.parse(await page.getByTestId('saved').textContent());
    assert.equal(saved.failure_notification, 'feishu');
    assert.deepEqual(errors, []);
  });

  await test('wechat reminder uses its own configured provider', async t => {
    const { page, errors } = await fixture(t, { wechat: true });
    const select = page.getByLabel('失败提醒');
    assert.equal(await select.locator('option[value="feishu"]').evaluate(option => option.disabled), true);
    assert.equal(await select.locator('option[value="wechat"]').evaluate(option => option.disabled), false);
    await select.selectOption('wechat');
    await page.getByRole('button', { name: '保存更改' }).click();
    assert.equal(JSON.parse(await page.getByTestId('saved').textContent()).failure_notification, 'wechat');
    assert.deepEqual(errors, []);
  });

  await test('missing bot cannot be newly selected and does not block saving', async t => {
    const { page } = await fixture(t);
    await page.getByText('在 Settings → IM 配置飞书或微信 Bot 后，可选择失败提醒。', { exact: true }).waitFor();
    const option = page.getByLabel('失败提醒').locator('option[value="feishu"]');
    assert.equal(await option.evaluate(element => element.disabled), true, await option.evaluate(element => element.outerHTML));
    await page.getByRole('button', { name: '保存更改' }).click();
    assert.equal(JSON.parse(await page.getByTestId('saved').textContent()).failure_notification, null);
  });

  for (const unavailable of [false, true]) {
    await test(`synced selection survives editing when bot ${unavailable ? 'settings cannot be read' : 'is missing'}`, async t => {
      const { page, errors } = await fixture(t, { selected: true, unavailable });
      assert.equal(await page.getByLabel('失败提醒').inputValue(), 'feishu');
      await page.getByText(unavailable ? '无法读取 IM Bot 配置，已有提醒设置仍会保留。' : '当前环境未配置飞书 Bot，已保留提醒设置；配置完成后，后续执行失败时会发送提醒。', { exact: true }).waitFor();
      await page.getByLabel('名称', { exact: true }).fill('修改任务名称');
      await page.getByRole('button', { name: '保存更改' }).click();
      const saved = JSON.parse(await page.getByTestId('saved').textContent());
      assert.equal(saved.name, '修改任务名称');
      assert.equal(saved.failure_notification, 'feishu');
      assert.deepEqual(errors, []);
    });
  }

  await test('synced reminder can be explicitly removed without a local bot', async t => {
    const { page } = await fixture(t, { selected: true });
    await page.getByLabel('失败提醒').selectOption('');
    await page.getByRole('button', { name: '保存更改' }).click();
    assert.equal(JSON.parse(await page.getByTestId('saved').textContent()).failure_notification, null);
  });
} finally {
  await browser?.close();
  await server.close();
}
