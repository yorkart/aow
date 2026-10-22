import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const first = 'g123456789ab';
const second = 'g123456789ac';
const endpoint = '/api/aow/settings/configuration';
const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  async function fixture(t, width = 1280, withProjects = false, slowRestore = false) {
    const context = await browser.newContext({ viewport: { width, height: 800 } });
    await context.addInitScript(() => { try { localStorage.setItem('aow.language', 'zh-CN'); } catch { /* Explicit Chinese fixture with blocked-storage coverage. */ } });
    const initial = { config_repo: '/repo', config_id: first };
    const state = { selection: initial, active: initial, saves: [], errors: [], failSave: false, projectsAvailable: withProjects, releaseRestore: undefined };
    const restoreGate = slowRestore ? new Promise(resolve => { state.releaseRestore = resolve; }) : Promise.resolve();
    t.after(async () => { state.releaseRestore?.(); await context.close(); assert.deepEqual(state.errors, []); });
    await context.route('**/api/**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      let data = [];
      if (url.pathname === '/api/auth/status') data = { configured: true, authenticated: true };
      else if (url.pathname === '/api/aow/settings') data = { notes_base: '/notes', execution_path: ['/usr/bin'], editor: { word_wrap: false }, node_addresses: [] };
      else if (url.pathname === '/api/aow/projects' && state.projectsAvailable) data = ['one', 'two'].map(id => ({
        id, name: id, registered_path: `/repo/${id}`, common_git_dir: `/repo/${id}/.git`, notes_path: '/notes',
        worktrees: [{ id: `wt-${id}`, project_id: id, path: `/repo/${id}`, branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false, color: 'default' }],
      }));
      else if (url.pathname === '/api/fs/text/repo/one/restored.txt') {
        await restoreGate;
        data = { path: '/repo/one/restored.txt', content: 'Restored content', version: '1', language: 'plaintext', mime: 'text/plain' };
      }
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
    await page.addInitScript(() => { try { localStorage.setItem('aow.language', 'zh-CN'); } catch { /* Explicit Chinese fixture with blocked-storage coverage. */ } });
    page.on('pageerror', error => state.errors.push(error.message));
    if (withProjects) await page.addInitScript(() => localStorage.setItem('aow-active', '/repo/one'));
    if (slowRestore) await page.addInitScript(() => localStorage.setItem('aow-workspace-tabs:/repo/one', JSON.stringify({
      documents: [{ id: '/repo/one/restored.txt', path: '/repo/one/restored.txt', name: 'restored.txt', kind: 'text', explorerSource: 'project' }],
      active: 'document:/repo/one/restored.txt',
    })));
    await page.goto(`${base}/aow/?ui=desktop`);
    const open = async () => {
      await page.getByRole('button', { name: "设置", exact: true }).click();
      const dialog = page.getByRole('region', { name: '设置', exact: true });
      await dialog.getByRole('button', { name: /^配置/ }).click();
      await dialog.getByRole('radio').first().waitFor();
      return dialog;
    };
    return { page, state, open, dialog: await open() };
  }

  await test('Settings is a singleton project tab and retains drafts when moved', async t => {
    const { page, dialog } = await fixture(t, 1280, true);
    const settings = page.getByRole('button', { name: "设置", exact: true });
    const draft = dialog.getByRole('textbox', { name: '配置仓库目录' });
    await draft.fill('/unsaved');
    const firstSurface = page.locator('.project-aow-surface').filter({ has: dialog });
    await settings.click();
    assert.equal(await page.locator('.project-aow-settings-slot .project-aow-settings').count(), 1);
    assert.equal(await draft.inputValue(), '/unsaved');
    await firstSurface.evaluate(element => { element.dataset.originalSettingsOwner = 'true'; });
    await page.locator('button[title="/repo/two"]').first().click();
    assert.equal(await dialog.isVisible(), false);
    await settings.click();
    await dialog.waitFor();
    assert.equal(await draft.inputValue(), '/unsaved');
    assert.equal(await page.locator('.project-aow-settings-slot .project-aow-settings').count(), 1);
    assert.equal(await page.locator('[data-original-settings-owner] .project-aow-settings').count(), 0);
    assert.equal(await page.getByRole('dialog').count(), 0);
    page.once('dialog', prompt => prompt.dismiss());
    await page.getByRole('button', { name: '关闭 Settings', exact: true }).click();
    assert.equal(await dialog.isVisible(), true);
    page.once('dialog', prompt => prompt.accept());
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
  });

  await test('finishing workspace restoration does not replace Settings selected during loading', async t => {
    const { page, state, dialog } = await fixture(t, 1280, true, true);
    assert.equal(await dialog.isVisible(), true);
    state.releaseRestore();
    await page.waitForFunction(() => JSON.parse(localStorage.getItem('aow-workspace-tabs:/repo/one') || '{}').active === 'settings');
    assert.equal(await dialog.isVisible(), true);
    assert.equal(await page.getByRole('tab', { name: "设置" }).getAttribute('aria-selected'), 'true');
  });

  await test('standalone Settings becomes a project tab without covering later project selections', async t => {
    const { page, state, dialog } = await fixture(t, 1280, false, true);
    const draft = dialog.getByRole('textbox', { name: '配置仓库目录' });
    await draft.fill('/unsaved');
    state.projectsAvailable = true;
    await page.getByRole('button', { name: '刷新全部', exact: true }).click();
    await page.locator('.project-aow-settings-slot .project-aow-settings').waitFor();
    state.releaseRestore();
    await page.waitForFunction(() => JSON.parse(localStorage.getItem('aow-workspace-tabs:/repo/one') || '{}').active === 'settings');
    assert.equal(await page.locator('.project-aow-settings-standalone').count(), 0);
    assert.equal(await draft.inputValue(), '/unsaved');
    await page.locator('button[title="/repo/two"]').first().click();
    await dialog.waitFor({ state: 'hidden' });
    await page.getByRole('button', { name: "设置", exact: true }).click();
    await dialog.waitFor();
    assert.equal(await draft.inputValue(), '/unsaved');
  });

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

  await test('configuration paths normalize and preselect, while invalid and empty repositories prevent saving', async t => {
    const { page, state, dialog } = await fixture(t, 760);
    const input = dialog.getByRole('textbox', { name: '配置仓库目录' });
    const read = dialog.getByRole('button', { name: '读取版本', exact: true });
    const save = dialog.getByRole('button', { name: '保存', exact: true });
    await input.fill('/not-git'); await read.click();
    await dialog.getByRole('alert').filter({ hasText: '路径不是 Git 仓库' }).waitFor();
    assert.equal(await save.isDisabled(), true);
    await input.fill('/empty'); await read.click();
    await dialog.getByRole('status').filter({ hasText: '没有配置版本目录' }).waitFor();
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
    await dialog.getByRole('button', { name: /^笔记/ }).click();
    await dialog.getByRole('button', { name: /^配置/ }).click();
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
