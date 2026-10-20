import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { chromium } from 'playwright';
import { themePreviewServer } from './fixtures/theme-preview-server.mjs';

const server = await themePreviewServer();
const browser = await chromium.launch({ headless: true, ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
const previewPath = '/tests/theme-preview.html';

async function fixture(t, options = {}) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, ...options });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  t.after(async () => { await context.close(); assert.deepEqual(errors, []); });
  page.setDefaultTimeout(20000);
  return { context, page };
}

async function selected(page, id) {
  await page.waitForFunction(id => document.documentElement.dataset.theme === id, id);
}

try {
  await test('all themes update the UI, editor, diff, ANSI palette and Mermaid without losing live state', async t => {
    const { page } = await fixture(t);
    await page.goto(server.url + previewPath);
    await page.waitForFunction(() => window.themeEditor && window.themeDiff);
    await page.locator('.project-aow-snapshot-flowchart svg').waitFor();
    await page.locator('.xterm-rows').getByText(/terminal stays connected/).waitFor();
    const baseline = { connections: server.state.connections, claims: server.state.claims };
    await page.evaluate(() => {
      window.originalThemeEditor = window.themeEditor;
      window.originalThemeDiff = window.themeDiff;
      window.themeEditor.pushUndoStop();
      window.themeEditor.executeEdits('test', [{ range: { startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 1 }, text: '// unsaved edit\n' }]);
      window.themeEditor.pushUndoStop();
    });
    const themes = await page.evaluate(async () => (await import('/src/lib/theme/themes.ts')).themes);
    for (const theme of themes) {
      await page.getByRole('radio', { name: `${theme.name} ${theme.family}`, exact: false }).check();
      await selected(page, theme.id);
      await page.waitForFunction(expected => {
        const background = document.querySelector('.monaco-editor-background');
        const rgb = color => {
          const element = document.createElement('span'); element.style.color = color;
          document.body.append(element); const value = getComputedStyle(element).color; element.remove(); return value;
        };
        return background && getComputedStyle(background).backgroundColor === rgb(expected);
      }, theme.editor.background);
      assert.equal(await page.evaluate(() => window.themeEditor === window.originalThemeEditor && window.themeDiff === window.originalThemeDiff), true);
      assert.match(await page.evaluate(() => window.themeEditor.getValue()), /^\/\/ unsaved edit\n/);
      assert.equal(await page.locator('.xterm-rows').getByText(/terminal stays connected/).count(), 1);
      const terminal = await page.locator('.xterm-rows').evaluate(element => ({
        background: getComputedStyle(element.closest('.xterm').querySelector('.xterm-scrollable-element')).backgroundColor,
        colors: [...element.querySelectorAll('span')].filter(span => span.textContent.trim()).map(span => getComputedStyle(span).color),
      }));
      const expected = await page.evaluate(theme => {
        const rgb = color => {
          const element = document.createElement('span'); element.style.color = color;
          document.body.append(element); const value = getComputedStyle(element).color; element.remove(); return value;
        };
        return { background: rgb(theme.terminal.background), red: rgb(theme.terminal.red), green: rgb(theme.terminal.green) };
      }, theme);
      assert.equal(terminal.background, expected.background);
      assert.ok(terminal.colors.includes(expected.red), `${theme.id} ANSI red`);
      assert.ok(terminal.colors.includes(expected.green), `${theme.id} ANSI green`);
      await page.locator('.project-aow-snapshot-flowchart svg').waitFor();
      assert.equal(await page.locator('.project-aow-snapshot-flowchart-error').count(), 0);
      assert.equal(await page.evaluate(() => document.documentElement.style.colorScheme), theme.appearance);
      assert.deepEqual({ connections: server.state.connections, claims: server.state.claims }, baseline, 'theme changes must not reconnect the terminal');
    }
    await page.evaluate(async () => { await window.themeEditor.getModel().undo(); });
    assert.doesNotMatch(await page.evaluate(() => window.themeEditor.getValue()), /^\/\/ unsaved edit\n/);
  });

  await test('desktop and mobile settings expose themes, persist selection and synchronize other tabs', async t => {
    const { context, page } = await fixture(t);
    await page.goto(server.url + '/?ui=desktop');
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.getByRole('button', { name: /^Appearance/ }).click();
    assert.equal(await page.getByRole('radio').count(), 6);
    await page.getByRole('radio', { name: /^JetBrains Light / }).check();
    await selected(page, 'jetbrains-light');
    await page.reload();
    await selected(page, 'jetbrains-light');
    const mobile = await context.newPage();
    await mobile.setViewportSize({ width: 390, height: 844 });
    await mobile.goto(server.url + '/?ui=mobile');
    await mobile.getByRole('button', { name: '设置', exact: true }).click();
    await mobile.getByRole('button', { name: /^Appearance/ }).click();
    assert.equal(await mobile.getByRole('radio', { name: /^JetBrains Light / }).isChecked(), true);
    await mobile.getByRole('radio', { name: /^VS Code Dark Modern / }).check();
    await selected(mobile, 'vscode-dark-modern');
    await selected(page, 'vscode-dark-modern');
    assert.equal(await mobile.locator('.appearance-settings').evaluate(element => element.scrollWidth <= element.clientWidth), true);
    await mobile.getByRole('radio', { name: /^VS Code Dark Modern / }).focus();
    await mobile.keyboard.press('ArrowLeft');
    await selected(mobile, 'jetbrains-islands-light');
    await selected(page, 'jetbrains-islands-light');
    await mobile.evaluate(() => localStorage.removeItem('aow.theme'));
    await selected(page, 'aow-dark');
  });

  await test('invalid or unavailable storage falls back safely and subpath deployments stay isolated', async t => {
    const { context, page } = await fixture(t);
    await page.addInitScript(() => localStorage.setItem('aow.theme', 'removed-theme'));
    await page.goto(server.url + previewPath);
    await selected(page, 'aow-dark');
    const scoped = await context.newPage();
    await scoped.route('**/tests/theme-preview.html', async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: (await response.text()).replace('<head>', '<head><meta name="aow-base-path" content="/other">') });
    });
    // No live terminal is needed for this storage-only scenario.
    await scoped.routeWebSocket('**/other/api/**', socket => socket.close());
    await scoped.goto(server.url + previewPath);
    await scoped.getByRole('radio', { name: /^JetBrains Dark / }).check();
    assert.equal(await scoped.evaluate(() => localStorage.getItem('aow@/other:aow.theme')), 'jetbrains-dark');
    assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), 'aow-dark');
    const denied = await context.newPage();
    await denied.addInitScript(() => {
      Storage.prototype.getItem = () => { throw new DOMException('Blocked', 'SecurityError'); };
      Storage.prototype.setItem = () => { throw new DOMException('Blocked', 'SecurityError'); };
    });
    await denied.goto(server.url + previewPath);
    await selected(denied, 'aow-dark');
    await denied.getByRole('radio', { name: /^JetBrains Light / }).check();
    await selected(denied, 'jetbrains-light');
    await denied.getByRole('status').filter({ hasText: '浏览器阻止了保存' }).waitFor();
  });

  await test('default CSS matches the theme contract and every palette produces valid colors', async t => {
    const { page } = await fixture(t);
    await page.goto(server.url + previewPath);
    const result = await page.evaluate(async () => {
      const { themes } = await import('/src/lib/theme/themes.ts');
      const { themeTokens } = await import('/src/lib/theme/tokens.ts');
      return {
        defaults: themeTokens(themes[0]),
        invalid: themes.flatMap(theme => Object.entries(themeTokens(theme))
          .filter(([, value]) => !CSS.supports('color', value)).map(([name]) => `${theme.id}: ${name}`)),
      };
    });
    assert.deepEqual(result.invalid, []);
    const css = await readFile(new URL('../src/lib/theme/defaults.css', import.meta.url), 'utf8');
    for (const [name, value] of Object.entries(result.defaults)) assert.ok(css.includes(`${name}:${value};`), name);
  });
} finally {
  await browser.close();
  await server.close();
}
