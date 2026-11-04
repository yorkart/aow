import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}/tests/choice-input-preview.html`;
  for (const mobile of [false, true]) await test(`${mobile ? 'mobile' : 'desktop'} choices keep native validation and keyboard semantics with custom themed states`, async () => {
    const page = await browser.newPage({ viewport: { width: mobile ? 360 : 1000, height: 900 } });
    try {
      await page.goto(`${base}${mobile ? '?mobile' : ''}`);
      const radios = page.getByRole('radio');
      const checkbox = page.getByRole('checkbox', { name: 'Enable automation', exact: true });
      const temporary = page.getByRole('radio', { name: 'Temporary workspace', exact: true });
      const dynamic = page.getByRole('radio', { name: 'Dynamic workspace', exact: true });
      const state = control => control.evaluate(element => {
        const style = getComputedStyle(element);
        const mark = getComputedStyle(element, '::before');
        return { appearance: style.appearance, width: style.width, height: style.height, background: style.backgroundColor, radius: style.borderRadius, mark: mark.transform, opacity: style.opacity };
      });
      assert.equal(await radios.count(), 4);
      assert.equal(await page.locator('form').evaluate(form => form.checkValidity()), false);
      const empty = await state(temporary);
      assert.equal(empty.appearance, 'none');
      assert.equal(empty.width, mobile ? '20px' : '14px');
      assert.equal(empty.height, empty.width);
      assert.equal(empty.radius, '50%');
      assert.equal(empty.mark, 'matrix(0, 0, 0, 0, 0, 0)');
      await temporary.focus();
      await page.keyboard.press('ArrowRight');
      assert.equal(await dynamic.isChecked(), true);
      assert.equal(await temporary.isChecked(), false);
      assert.notEqual((await state(dynamic)).background, empty.background);
      await checkbox.focus();
      await page.keyboard.press('Space');
      assert.equal(await checkbox.isChecked(), true);
      assert.equal((await state(checkbox)).appearance, 'none');
      assert.notEqual((await state(checkbox)).mark, 'matrix(0, 0, 0, 0, 0, 0)');
      assert.equal(await page.getByRole('checkbox', { name: 'Mixed setting', exact: true }).evaluate(element => element.indeterminate), true);
      assert.equal(await page.getByRole('checkbox', { name: 'Inherited disabled setting', exact: true }).isDisabled(), true);
      assert.equal((await state(page.getByRole('checkbox', { name: 'Disabled checked setting', exact: true }))).opacity, '0.5');
      const card = page.locator('.theme-card > input');
      assert.equal(await card.evaluate(element => getComputedStyle(element).opacity), '0');
      assert.ok((await card.boundingBox()).width > 200, 'theme-card input retains its full-card hit area');
      await page.getByRole('button', { name: 'Save choices' }).click();
      assert.equal(await page.evaluate(() => document.body.dataset.submitted), 'true');
      const dark = (await state(checkbox)).background;
      await page.evaluate(async () => {
        const { themes } = await import('/src/lib/theme/themes.ts');
        const { setTheme } = await import('/src/lib/theme/store.ts');
        setTheme(themes.find(theme => theme.appearance === 'light').id);
      });
      assert.notEqual((await state(checkbox)).background, dark, 'control colors follow the active theme');
      await page.emulateMedia({ forcedColors: 'active' });
      assert.equal(await checkbox.evaluate(element => getComputedStyle(element).forcedColorAdjust), 'none');
      assert.notEqual((await state(checkbox)).background, (await state(temporary)).background);
    } finally { await page.close(); }
  });
} finally { await browser?.close(); await server.close(); }
