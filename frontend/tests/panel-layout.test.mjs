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
  const panel = (page, name) => page.getByRole('region', { name, exact: true });
  const header = (page, name) => panel(page, name).locator(':scope > [data-panel-anchor] > header');
  const toggle = (page, name) => panel(page, name).getByRole('button', { name: new RegExp(`^(展开|收起) ${name}$`) });
  const navigate = (page, name) => page.getByRole('button', { name: `定位 ${name}`, exact: true }).click();
  const viewport = page => page.locator('.aow-panel-viewport');
  async function fixture(t, { nested = false } = {}) {
    const page = await browser.newPage({ viewport: { width: 900, height: 640 }, reducedMotion: 'reduce' });
    page.setDefaultTimeout(8000);
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/panel-layout-preview.html${nested ? '?nested' : ''}`);
    await header(page, 'Beta').waitFor();
    return page;
  }
  async function settled(page) { await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))); }
  async function clickable(locator) {
    return locator.evaluate(element => { const box = element.getBoundingClientRect(); return element.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)); });
  }

  await test('panels use natural heights; empty defaults and manual choices survive refresh; collapsed content stays mounted', async t => {
    const page = await fixture(t);
    assert.equal(await toggle(page, 'Empty').getAttribute('aria-expanded'), 'false');
    await page.getByRole('button', { name: 'Toggle short content', exact: true }).click();
    await settled(page);
    const betaHeight = (await panel(page, 'Beta').boundingBox()).height;
    assert.ok(betaHeight < 100);
    await page.getByLabel('Preserved input').fill('keep this');
    await toggle(page, 'Alpha').click();
    assert.equal((await panel(page, 'Beta').boundingBox()).height, betaHeight);
    await page.getByRole('button', { name: 'Refresh Alpha', exact: true }).click();
    assert.equal(await page.getByLabel('Refresh count').textContent(), '1');
    assert.equal(await toggle(page, 'Alpha').getAttribute('aria-expanded'), 'false');
    await toggle(page, 'Alpha').press('Enter');
    assert.equal(await page.getByLabel('Preserved input').inputValue(), 'keep this');
    await page.getByRole('button', { name: 'Toggle empty data', exact: true }).click();
    assert.equal(await toggle(page, 'Empty').getAttribute('aria-expanded'), 'true');
    await toggle(page, 'Empty').click();
    await page.getByRole('button', { name: 'Toggle empty data', exact: true }).click();
    await page.getByRole('button', { name: 'Refresh Alpha', exact: true }).click();
    assert.equal(await toggle(page, 'Empty').getAttribute('aria-expanded'), 'false');
    await navigate(page, 'Empty');
    await panel(page, 'Empty').getByText('No items', { exact: true }).waitFor();
    assert.equal(await toggle(page, 'Empty').getAttribute('aria-expanded'), 'true');
  });

  await test('offscreen titles dock at both edges; navigation reveals content and respects the scroll limit', async t => {
    const page = await fixture(t);
    await settled(page);
    assert.equal(await header(page, 'Beta').getAttribute('data-docked'), 'bottom');
    assert.equal(await clickable(header(page, 'Beta')), true);
    assert.equal(await clickable(header(page, 'Gamma')), true);
    assert.equal(await clickable(header(page, 'Empty')), true);
    await navigate(page, 'Beta');
    await settled(page);
    const box = await viewport(page).boundingBox();
    const alpha = await header(page, 'Alpha').boundingBox(), beta = await header(page, 'Beta').boundingBox();
    assert.ok(Math.abs(alpha.y - box.y) < 1);
    assert.ok(Math.abs(beta.y - box.y - alpha.height) < 1);
    assert.equal(await header(page, 'Alpha').getAttribute('data-docked'), 'top');
    assert.equal(await clickable(header(page, 'Alpha')), true);
    await page.getByRole('button', { name: 'Refresh Alpha', exact: true }).click();
    assert.equal(await page.getByLabel('Refresh count').textContent(), '1');
    assert.equal(await toggle(page, 'Alpha').getAttribute('aria-expanded'), 'true');
    await navigate(page, 'Gamma');
    await settled(page);
    const scroll = await viewport(page).evaluate(element => ({ top: element.scrollTop, max: element.scrollHeight - element.clientHeight }));
    assert.ok(Math.abs(scroll.top - scroll.max) < 1, 'short final panels clamp to the end without blank filler');
    await navigate(page, 'Alpha');
    await settled(page);
    assert.equal(await viewport(page).evaluate(element => element.scrollTop), 0);
    assert.equal(await page.locator('[data-panel-anchor]').count(), 4, 'docking does not duplicate headers or controls');
    const dimensions = await page.locator('.aow-icon-button').evaluateAll(elements => elements.map(element => {
      const box = element.getBoundingClientRect(), svg = element.querySelector('svg').getBoundingClientRect();
      return [box.width, box.height, svg.width, svg.height, getComputedStyle(element).padding];
    }));
    assert.ok(dimensions.every(value => JSON.stringify(value) === '[22,22,13,13,"0px"]'), JSON.stringify(dimensions));
  });

  await test('nested panels and short windows retain navigation without covering the content', async t => {
    const page = await fixture(t);
    await page.getByRole('button', { name: 'Toggle many panels', exact: true }).click();
    const jump = page.getByRole('combobox', { name: '快速定位面板', exact: true });
    await jump.waitFor();
    await jump.selectOption({ label: 'Nested 12' });
    await settled(page);
    assert.equal(await clickable(header(page, 'Nested 12')), true);
    await toggle(page, 'Project 12').click();
    await settled(page);
    assert.equal(await jump.locator('option').filter({ hasText: /^Nested 12$/ }).count(), 0);
    await page.getByRole('button', { name: 'Toggle many panels', exact: true }).click();
    await jump.waitFor({ state: 'hidden' });
    await page.setViewportSize({ width: 900, height: 180 });
    await jump.waitFor();
    await jump.selectOption({ label: 'Beta' });
    await settled(page);
    assert.equal(await clickable(header(page, 'Beta')), true);
    await page.setViewportSize({ width: 900, height: 640 });
    await jump.waitFor({ state: 'hidden' });
    await settled(page);
    assert.equal(await clickable(header(page, 'Alpha')), true);
  });

  await test('docked titles stay still during scrolling, including before the next layout callback', async t => {
    const page = await fixture(t);
    await viewport(page).evaluate(element => { element.scrollTop = 120; });
    await settled(page);
    const samples = await viewport(page).evaluate(element => {
      const headers = [...element.querySelectorAll('[data-docked]')].filter(header => header.dataset.docked);
      const before = headers.map(header => header.getBoundingClientRect().top);
      // Compositor scrolling can paint before JS runs. Sample within the same
      // task so a transform applied on a later animation frame cannot hide lag.
      return [180, 300, 450, 600, 420, 240, 120].map(top => {
        element.scrollTop = top;
        return headers.map((header, index) => header.getBoundingClientRect().top - before[index]);
      });
    });
    assert.ok(samples.flat().every(delta => Math.abs(delta) < .5), `docked titles moved with the content: ${JSON.stringify(samples)}`);
    const before = await header(page, 'Alpha').boundingBox();
    const box = await viewport(page).boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.wheel(0, 240);
    await page.waitForFunction(() => document.querySelector('.aow-panel-viewport').scrollTop > 120);
    await settled(page);
    assert.equal((await header(page, 'Alpha').boundingBox()).y, before.y);
    assert.equal(await clickable(header(page, 'Alpha')), true);
    assert.equal(await clickable(header(page, 'Gamma')), true);
    const scrollTop = await viewport(page).evaluate(element => element.scrollTop);
    await header(page, 'Alpha').hover();
    await page.mouse.wheel(0, 100);
    await page.waitForFunction(top => document.querySelector('.aow-panel-viewport').scrollTop > top, scrollTop);
    await settled(page);
    assert.equal((await header(page, 'Alpha').boundingBox()).y, before.y);
  });

  await test('docked nested titles follow their sidebar bounds and disappear when their parent collapses', async t => {
    const page = await fixture(t, { nested: true });
    await viewport(page).evaluate(element => { element.scrollTop = 120; });
    await settled(page);
    const before = await header(page, 'Alpha').boundingBox();
    await page.locator('.aow-panel-stack').evaluate(element => {
      element.parentElement.style.transform = 'translate(140px, 10px)';
      element.parentElement.style.width = '240px';
    });
    await settled(page);
    const after = await header(page, 'Alpha').boundingBox();
    assert.equal(after.x, before.x + 140);
    assert.equal(after.y, before.y + 10);
    assert.equal(after.width, await viewport(page).evaluate(element => element.clientWidth));
    assert.equal(await clickable(header(page, 'Beta')), true);
    await toggle(page, 'Parent').click();
    await settled(page);
    assert.equal(await header(page, 'Alpha').isVisible(), false);
    assert.equal(await header(page, 'Beta').isVisible(), false);
    await navigate(page, 'Parent');
    await settled(page);
    assert.equal(await clickable(header(page, 'Beta')), true);
  });
} finally { await browser?.close(); await server.close(); }
