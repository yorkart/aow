import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'],
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}/tests/i18n-preview.html`;
  await test('English default ignores browser Chinese, selected language survives reload and reaches requests', async () => {
    const context = await browser.newContext({ locale: 'zh-CN' });
    try {
      const page = await context.newPage();
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.route('**/api/i18n-probe', route => route.fulfill({ json: {} }));
      await page.goto(base);
      await page.getByRole('heading', { name: 'Sign in to AoW', exact: true }).waitFor();
      assert.equal(await page.locator('html').getAttribute('lang'), 'en-US');
      const editorLabel = () => page.evaluate(async () => {
        await import('/src/features/editor/monaco.ts');
        const { localize } = await import('/node_modules/monaco-editor/esm/vs/nls.js');
        return localize(21, 'Close');
      });
      assert.match(await editorLabel(), /^Close$/);
      await page.locator('button[aria-haspopup="listbox"]').click();
      await page.getByRole('option', { name: '简体中文', exact: true }).click();
      page.once('dialog', dialog => dialog.accept());
      await Promise.all([page.waitForEvent('load'), page.getByRole('button', { name: 'Apply language' }).click()]);
      await page.getByRole('heading', { name: '登录 AoW', exact: true }).waitFor();
      assert.equal(await page.locator('html').getAttribute('lang'), 'zh-CN');
      assert.match(await editorLabel(), /^关闭$/);
      const request = page.waitForRequest('**/api/i18n-probe');
      await page.getByRole('button', { name: '发送', exact: true }).click();
      assert.equal((await request).headers()['accept-language'], 'zh-CN');
      await page.reload();
      await page.getByRole('heading', { name: '登录 AoW', exact: true }).waitFor();
      assert.deepEqual(errors, []);
    } finally { await context.close(); }
  });
  for (const width of [1000, 320]) await test(`custom language picker supports keyboard, outside dismissal and dialog Escape at ${width}px`, async () => {
    const context = await browser.newContext({ viewport: { width, height: 700 } });
    try {
      const page = await context.newPage();
      await page.goto(base);
      const picker = page.locator('button[aria-haspopup="listbox"]');
      const menu = page.getByRole('listbox');
      const apply = page.getByRole('button', { name: 'Apply language', exact: true });
      assert.equal(await page.locator('select').count(), 0);
      assert.equal(await apply.isDisabled(), true);
      await picker.focus();
      await page.keyboard.press('ArrowDown');
      await page.getByRole('option', { name: 'English', exact: true }).waitFor();
      assert.equal(await page.getByRole('option', { selected: true }).innerText(), 'English');
      await page.keyboard.press('End');
      assert.equal(await page.getByRole('option', { name: '简体中文', exact: true }).evaluate(node => node === document.activeElement), true);
      await page.keyboard.press('Escape');
      assert.equal(await menu.count(), 0);
      assert.equal(await picker.evaluate(node => node === document.activeElement), true);
      assert.equal(await page.evaluate(() => document.body.dataset.settingsEscape), undefined);
      await page.keyboard.press('ArrowDown');
      await page.keyboard.press('End');
      await page.keyboard.press('Enter');
      assert.equal(await apply.isEnabled(), true);
      assert.match(await picker.innerText(), /简体中文/);
      assert.equal(await page.locator('html').getAttribute('lang'), 'en-US', 'selection requires explicit application');
      await picker.click();
      assert.equal(await page.getByRole('option', { selected: true }).innerText(), '简体中文');
      const bounds = await menu.boundingBox();
      assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= width);
      await page.getByRole('heading').click();
      assert.equal(await menu.count(), 0);
    } finally { await context.close(); }
  });
  await test('blocked storage reports failure without reloading or losing the selected language', async () => {
    const context = await browser.newContext();
    try {
      await context.addInitScript(() => { Storage.prototype.setItem = () => { throw new Error('blocked'); }; });
      const page = await context.newPage();
      await page.goto(base);
      await page.locator('button[aria-haspopup="listbox"]').click();
      await page.getByRole('option', { name: '简体中文', exact: true }).click();
      page.once('dialog', dialog => dialog.accept());
      await page.getByRole('button', { name: 'Apply language' }).click();
      await page.getByRole('alert').waitFor();
      assert.match(await page.getByRole('alert').innerText(), /blocked saving the language/);
      assert.equal(await page.locator('html').getAttribute('lang'), 'en-US');
      assert.match(await page.locator('button[aria-haspopup="listbox"]').innerText(), /简体中文/);
    } finally { await context.close(); }
  });
} finally { await browser?.close(); await server.close(); }
