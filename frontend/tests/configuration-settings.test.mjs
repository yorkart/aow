import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const first = '550e8400-e29b-41d4-a716-446655440000';
const second = '550e8400-e29b-41d4-a716-446655440001';
const endpoint = '/api/aow/settings/configuration';
const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  async function fixture(t, width = 1280) {
    const context = await browser.newContext({ viewport: { width, height: 800 } });
    const initial = { config_repo: '/repo', config_id: first };
    const state = { selection: initial, active: initial, saves: [], errors: [], failSave: false };
    t.after(async () => { await context.close(); assert.deepEqual(state.errors, []); });
    await context.route('**/api/**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      let data = [];
      if (url.pathname === '/api/auth/status') data = { configured: true, authenticated: true };
      else if (url.pathname === '/api/aow/settings') data = { notes_base: '/notes', execution_path: ['/usr/bin'], editor: { word_wrap: false }, node_addresses: [] };
      else if (url.pathname === endpoint) {
        let changed = false;
        if (request.method() === 'PUT') {
          state.saves.push(request.postDataJSON());
          if (state.failSave) { await route.fulfill({ status: 500, json: { message: '无法写入 config.toml' } }); return; }
          changed = JSON.stringify(state.selection) !== JSON.stringify(request.postDataJSON());
          state.selection = request.postDataJSON();
        }
        data = { selection: state.selection, active_selection: state.active, restart_required: JSON.stringify(state.active) !== JSON.stringify(state.selection), changed };
      } else if (url.pathname === `${endpoint}/versions`) {
        const path = url.searchParams.get('path');
        if (path === '/not-git') { await route.fulfill({ status: 400, json: { message: '路径不是 Git 仓库：/not-git' } }); return; }
        data = { config_repo: path.startsWith('/other') ? '/other' : path, config_ids: path === '/empty' ? [] : [first, second], selected_id: path === `/other/${second}` ? second : null };
      } else if (url.pathname.includes('/pinned-')) data = { paths: [], revision: 0 };
      else if (url.pathname === '/api/terminals/task-stops') { await route.fulfill({ contentType: 'text/event-stream', body: ': ready\n\n' }); return; }
      else if (url.pathname === '/api/terminals/agents') data = { agents: {}, titles: {}, processes: {} };
      await route.fulfill({ json: data });
    });
    const page = await context.newPage();
    page.on('pageerror', error => state.errors.push(error.message));
    await page.goto(`${base}/aow/?ui=desktop`);
    const open = async () => {
      await page.getByRole('button', { name: 'Settings', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: '设置', exact: true });
      await dialog.getByRole('button', { name: /^Configuration/ }).click();
      await dialog.getByRole('radio').first().waitFor();
      return dialog;
    };
    return { page, state, open, dialog: await open() };
  }

  await test('selection is single-choice, no-op saves skip restart messaging, changed saves persist and retain active version', async t => {
    const { state, dialog, open } = await fixture(t);
    assert.equal(await dialog.getByRole('textbox', { name: '配置仓库目录' }).inputValue(), '/repo');
    assert.equal(await dialog.getByRole('radio', { name: new RegExp(first) }).isChecked(), true);
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '配置未发生变化' }).waitFor();
    assert.equal(await dialog.getByText(/重启服务后生效/).count(), 0);
    await dialog.getByRole('radio', { name: second, exact: true }).check();
    assert.equal(await dialog.getByRole('radio', { name: new RegExp(first) }).isChecked(), false);
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '配置已保存，重启服务后生效' }).waitFor();
    assert.deepEqual(state.selection, { config_repo: '/repo', config_id: second });
    assert.deepEqual(state.active, { config_repo: '/repo', config_id: first });
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    const reopened = await open();
    assert.equal(await reopened.getByRole('radio', { name: new RegExp(second) }).isChecked(), true);
    await reopened.getByRole('status').filter({ hasText: '已保存的配置与当前运行版本不同' }).waitFor();
  });

  await test('UUID paths normalize and preselect, while invalid and empty repositories prevent saving', async t => {
    const { page, state, dialog } = await fixture(t, 760);
    const input = dialog.getByRole('textbox', { name: '配置仓库目录' });
    const read = dialog.getByRole('button', { name: '读取版本', exact: true });
    const save = dialog.getByRole('button', { name: '保存', exact: true });
    await input.fill('/not-git'); await read.click();
    await dialog.getByRole('alert').filter({ hasText: '路径不是 Git 仓库' }).waitFor();
    assert.equal(await save.isDisabled(), true);
    await input.fill('/empty'); await read.click();
    await dialog.getByRole('status').filter({ hasText: '没有 UUID 版本目录' }).waitFor();
    assert.equal(await save.isDisabled(), true);
    await input.fill('/other'); await read.click();
    await dialog.getByRole('radio').first().waitFor();
    assert.equal(await dialog.locator('input[type=radio]:checked').count(), 0);
    assert.equal(await save.isDisabled(), true);
    await input.fill(`/other/${second}`); await read.click();
    await dialog.getByRole('radio', { name: second, exact: true }).waitFor();
    assert.equal(await input.inputValue(), '/other');
    assert.equal(await dialog.getByRole('radio', { name: second, exact: true }).isChecked(), true);
    await save.click();
    await dialog.getByRole('status').filter({ hasText: '配置已保存' }).waitFor();
    assert.deepEqual(state.saves, [{ config_repo: '/other', config_id: second }]);
    assert.equal(await dialog.evaluate(el => el.scrollWidth <= el.clientWidth), true);
    await page.screenshot({ path: '/tmp/aow-configuration-settings.png' });
  });

  await test('failed saves retain edits and saved selection; switching sections preserves an unsaved draft', async t => {
    const { page, state, dialog } = await fixture(t);
    state.failSave = true;
    await dialog.getByRole('radio', { name: second, exact: true }).check();
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '无法写入 config.toml' }).waitFor();
    assert.equal(state.selection.config_id, first);
    await dialog.getByRole('button', { name: /^Notes/ }).click();
    await dialog.getByRole('button', { name: /^Configuration/ }).click();
    assert.equal(await dialog.getByRole('radio', { name: second, exact: true }).isChecked(), true);
    page.once('dialog', prompt => prompt.dismiss());
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    assert.equal(await dialog.isVisible(), true);
    state.failSave = false;
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '配置已保存' }).waitFor();
  });
} finally {
  await browser?.close();
  await server.close();
}
