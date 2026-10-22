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
      await page.getByRole('combobox').selectOption('zh-CN');
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
  await test('blocked storage reports failure without reloading or losing the selected language', async () => {
    const context = await browser.newContext();
    try {
      await context.addInitScript(() => { Storage.prototype.setItem = () => { throw new Error('blocked'); }; });
      const page = await context.newPage();
      await page.goto(base);
      await page.getByRole('combobox').selectOption('zh-CN');
      page.once('dialog', dialog => dialog.accept());
      await page.getByRole('button', { name: 'Apply language' }).click();
      await page.getByRole('alert').waitFor();
      assert.match(await page.getByRole('alert').innerText(), /blocked saving the language/);
      assert.equal(await page.locator('html').getAttribute('lang'), 'en-US');
      assert.equal(await page.getByRole('combobox').inputValue(), 'zh-CN');
    } finally { await context.close(); }
  });
} finally { await browser?.close(); await server.close(); }
