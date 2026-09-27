import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { makeTurn, session, snapshot } from './fixtures/session-snapshot.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  const baseURL = `http://127.0.0.1:${server.httpServer.address().port}/tests/session-snapshot-preview.html`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });

  async function open(t, mobile = false, width = mobile ? 390 : 1280) {
    const page = await browser.newPage({ viewport: { width, height: mobile ? 844 : 960 } });
    t.after(() => page.close());
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    t.after(() => assert.deepEqual(errors, []));
    let currentSnapshot = structuredClone(snapshot);
    await page.route('**/api/**', async (route) => {
      if (route.request().method() === 'GET' && new URL(route.request().url()).pathname.endsWith('/snapshot')) {
        await route.fulfill({ json: currentSnapshot });
      } else await route.fulfill({ status: 404, json: { message: 'No test fixture' } });
    });
    await page.goto(`${baseURL}${mobile ? '?mobile' : ''}`);
    await page.locator('.session-process-toggle').first().waitFor();
    return { page, updateSnapshot: (value) => { currentSnapshot = value; } };
  }
  const toggles = (page) => page.locator('.session-process-toggle');
  const expanded = (page) => toggles(page).evaluateAll((nodes) => nodes.map((node) => node.getAttribute('aria-expanded')));
  async function update(page, patch) { await page.evaluate((patch) => window.sessionPreview.update(patch), patch); }
  async function noOverflow(page) {
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    for (const element of await page.locator('.project-aow-snapshot-conversation, .mobile-conversation, .session-turn-process').all()) {
      assert.equal(await element.evaluate((node) => node.scrollWidth <= node.clientWidth + 1), true, 'conversation and process must fit the available width');
    }
  }

  for (const mobile of [false, true]) {
    await test(`${mobile ? 'mobile' : 'desktop'} displays exported native Hermes session data`, { skip: !process.env.AOW_HERMES_TEST_EXPORT }, async (t) => {
      const directory = process.env.AOW_HERMES_TEST_EXPORT;
      const nativeSession = JSON.parse(await readFile(join(directory, 'session.json'), 'utf8'));
      const nativeSnapshot = JSON.parse(await readFile(join(directory, 'snapshot.json'), 'utf8'));
      const { page, updateSnapshot } = await open(t, mobile);
      updateSnapshot(nativeSnapshot);
      await update(page, { session: nativeSession, snapshot: nativeSnapshot });
      await page.getByText(nativeSession.title, { exact: true }).first().waitFor();
      const turns = page.locator(mobile ? '.mobile-turn' : '.project-aow-snapshot-turn');
      await turns.last().getByText(nativeSnapshot.turns.at(-1).user.text, { exact: true }).waitFor();
      assert.equal(await turns.count(), nativeSnapshot.turns.length);
      const finals = page.locator(mobile ? '.mobile-message.assistant .mobile-markdown' : '.project-aow-snapshot-conclusion');
      assert.equal(await finals.count(), nativeSnapshot.turns.filter(turn => turn.final).length);
      assert.equal(await page.locator('.session-turn-pending.interrupted').count(), nativeSnapshot.turns.filter(turn => turn.status === 'interrupted' && !turn.final).length);
      for (const toggle of await page.locator('.session-process-toggle').all()) {
        if (await toggle.getAttribute('aria-expanded') === 'false') await toggle.click();
      }
      for (const group of await page.locator('.session-tool-group-toggle').all()) {
        if (await group.getAttribute('aria-expanded') === 'false') await group.click();
      }
      const tools = nativeSnapshot.turns.flatMap(turn => turn.activities).filter(activity => activity.kind === 'tool');
      assert.equal(await page.locator('.session-process-tool').count(), tools.length);
      assert.equal(await page.locator('.session-process-tool.failed').count(), tools.filter(tool => tool.status === 'failed').length);
      const toolDetails = page.locator('.session-tool-toggle');
      for (const toggle of await toolDetails.all()) await toggle.click();
      assert.equal(await page.locator('.session-tool-detail:visible').count(), tools.length);
      await noOverflow(page);
    });

    await test(`${mobile ? 'mobile' : 'desktop'} Hermes conversations show their own identity and native transcript`, async (t) => {
      const { page, updateSnapshot } = await open(t, mobile);
      const hermesSession = { ...session, id: 'hermes:hermes-session', session_id: 'hermes-session', agent: 'hermes', title: 'Hermes conversation' };
      const hermesSnapshot = { ...snapshot, agent: 'hermes', session_id: 'hermes-session', title: hermesSession.title,
        turns: [makeTurn('hermes-1', 'Read the repository', 'Hermes completed the task.', [{ kind: 'tool', text: 'terminal', status: 'completed' }])] };
      updateSnapshot(hermesSnapshot);
      await update(page, { session: hermesSession, snapshot: hermesSnapshot });
      await page.getByText('Hermes completed the task.', { exact: true }).waitFor();
      await page.getByText('Hermes', { exact: true }).first().waitFor();
      const icons = page.locator('img.agent-icon');
      assert.ok(await icons.count() > 0);
      assert.match(await icons.first().getAttribute('src'), /hermes/);
      await icons.first().evaluate(image => image.decode());
      assert.equal(await icons.first().evaluate(image => image.complete && image.naturalWidth > 0), true);
      assert.equal(await page.getByText('TraeCode CLI', { exact: true }).count(), 0);
      await noOverflow(page);
    });

    await test(`${mobile ? 'mobile' : 'desktop'} image references preview local attachments without changing message text`, async (t) => {
      const { page, updateSnapshot } = await open(t, mobile);
      const file = '/tmp/截图 #1 & example.png';
      const rawUrl = (path) => '/api/fs/raw' + path.split('/').map(encodeURIComponent).join('/');
      const prompt = [
        `<image name=[Image #1] path="${file}">`,
        '<image name=[Image #2] path="/tmp/missing.png">',
        '', '对照 [Image #1] 和 [Image #2]，未附图 [Image #9]。',
        '', '`[Image #1]`', '', '```text', '[Image #1]', '```',
        '', '[已有链接 [Image #1]](https://example.com/)',
      ].join('\n');
      const next = { ...snapshot, turns: [
        makeTurn('images-1', prompt, '回复中引用 [Image #1]。', []),
        makeTurn('images-2', '<image name=[Image #1] path="/tmp/second.png">\n\n第二轮 [Image #1]。', '第二轮回复 [Image #1]。', []),
      ] };
      const requested = [];
      await page.route('**/api/fs/raw/**', async (route) => {
        requested.push(new URL(route.request().url()).pathname);
        if (route.request().url().endsWith('missing.png')) return route.fulfill({ status: 404 });
        return route.fulfill({ contentType: 'image/svg+xml', body: '<svg xmlns="http://www.w3.org/2000/svg" width="800" height="400"><rect width="800" height="400" fill="#243b59"/><text x="40" y="210" fill="#edf5ff" font-size="44">Session image preview</text></svg>' });
      });
      if (mobile) {
        updateSnapshot(next);
        await page.getByRole('button', { name: '刷新', exact: true }).click();
      } else await update(page, { snapshot: next });
      const messages = page.locator(mobile ? '.mobile-message.user' : '.project-aow-snapshot-message.user');
      const first = messages.first();
      const links = first.locator('.session-image-reference');
      await links.first().waitFor();
      assert.equal(await links.count(), 2, 'only mapped references in prose become preview links');
      assert.match(await first.innerText(), /<image name=\[Image #1\] path="\/tmp\/截图 #1 & example.png">/);
      assert.equal(await first.locator('code .session-image-reference, a a').count(), 0);
      assert.equal(await links.first().getAttribute('href'), rawUrl(file));
      assert.equal(await messages.last().locator('.session-image-reference').getAttribute('href'), rawUrl('/tmp/second.png'));
      const assistant = page.locator(mobile ? '.mobile-message.assistant' : '.project-aow-snapshot-conclusion').first();
      assert.equal(await assistant.locator('.session-image-reference').getAttribute('href'), rawUrl(file));
      assert.deepEqual(requested, [], 'images load only when previewed');

      const height = await first.evaluate(node => node.getBoundingClientRect().height);
      await links.first().hover();
      const tooltip = page.getByRole('tooltip');
      await tooltip.locator('img.loaded').waitFor();
      assert.equal(await tooltip.locator('img').getAttribute('alt'), '[Image #1]');
      assert.deepEqual(requested, [rawUrl(file)]);
      assert.equal(await first.evaluate(node => node.getBoundingClientRect().height), height);
      assert.equal(await tooltip.evaluate(node => {
        const rect = node.getBoundingClientRect();
        return rect.left >= 0 && rect.right <= innerWidth && rect.top >= 0 && rect.bottom <= innerHeight;
      }), true, 'preview fits inside the viewport');
      await tooltip.hover();
      await page.waitForTimeout(200);
      assert.equal(await tooltip.isVisible(), true, 'moving into the preview keeps it open');
      if (process.env.SESSION_TEST_SCREENSHOTS) {
        await mkdir(process.env.SESSION_TEST_SCREENSHOTS, { recursive: true });
        await page.screenshot({ path: `${process.env.SESSION_TEST_SCREENSHOTS}/image-preview-${mobile ? 'mobile' : 'desktop'}.png` });
      }
      await page.keyboard.press('Escape');
      await tooltip.waitFor({ state: 'detached' });
      assert.equal(await links.first().getAttribute('aria-describedby'), null);

      await links.last().focus();
      await tooltip.getByText('图片无法加载，文件可能已移除或当前无法访问。').waitFor();
      await page.keyboard.press('Escape');
      await tooltip.waitFor({ state: 'detached' });
      await links.first().focus();
      await tooltip.locator('img.loaded').waitFor();
      await page.locator(mobile ? '.mobile-conversation' : '.project-aow-snapshot-conversation').evaluate(node => node.dispatchEvent(new Event('scroll')));
      await tooltip.waitFor({ state: 'detached' });
      await noOverflow(page);
    });
  }

  await test('image references update with snapshot content and stay plain in public views', async (t) => {
    const { page } = await open(t);
    const prompt = '&lt;image name=[Image #3] path=&quot;/tmp/escaped.png&quot;&gt;\n\n预览 [Image #3]，未知 [Image #4]。';
    const next = { ...snapshot, turns: [makeTurn('escaped-image', prompt, '查看 [Image #3]。', [])] };
    await update(page, { snapshot: next });
    const links = page.locator('.session-image-reference');
    await links.first().waitFor();
    assert.equal(await links.count(), 2);
    assert.equal(await links.first().getAttribute('href'), '/api/fs/raw/tmp/escaped.png');
    await update(page, { snapshot: { ...next, turns: [makeTurn('escaped-image', prompt.replace('escaped.png', 'updated.png'), '查看 [Image #3]。', [])] } });
    await page.waitForFunction(() => document.querySelector('.session-image-reference')?.getAttribute('href') === '/api/fs/raw/tmp/updated.png');
    assert.equal(await links.last().getAttribute('href'), '/api/fs/raw/tmp/updated.png');
    await update(page, { publicView: true });
    await links.first().waitFor({ state: 'detached' });
    assert.equal(await page.getByRole('tooltip').count(), 0);
    assert.match(await page.locator('.project-aow-snapshot-message.user').innerText(), /预览 \[Image #3\]/);
  });

  await test('finished turns hide progress, and refresh preserves manual choices and scroll', async (t) => {
    const { page } = await open(t);
    assert.deepEqual(await expanded(page), ['false', 'false']);
    assert.equal(await page.getByText('正在检查现有会话页面和数据结构。', { exact: true }).count(), 0);
    assert.equal(await page.locator('.project-aow-snapshot-conclusion').count(), 2);
    await toggles(page).first().click();
    assert.deepEqual(await expanded(page), ['true', 'false']);
    await toggles(page).last().click();
    const before = await page.locator('.project-aow-snapshot-conversation').evaluate((node) => { node.scrollTop = 80; return node.scrollTop; });
    await page.getByRole('button', { name: '刷新会话快照' }).click();
    assert.deepEqual(await expanded(page), ['true', 'true']);
    assert.equal(await page.locator('.project-aow-snapshot-conversation').evaluate((node) => node.scrollTop), before);
    await toggles(page).last().focus();
    await page.keyboard.press('Enter');
    assert.deepEqual(await expanded(page), ['true', 'false']);
    assert.equal(await page.evaluate(() => window.__unsafe), undefined);
    await noOverflow(page);
  });

  await test('a running round opens then collapses on completion; changing sessions resets choices', async (t) => {
    const { page } = await open(t);
    await toggles(page).first().click();
    const next = { ...snapshot, status: 'in_progress', turns: [...snapshot.turns, makeTurn('turn-3', '继续验证。', null, [
      { kind: 'commentary', text: '开始下一轮验证。' }, { kind: 'tool', text: 'exec_command', status: 'in_progress' },
    ], 'in_progress')] };
    await update(page, { snapshot: next });
    await page.locator('.project-aow-snapshot-turn').nth(2).waitFor();
    assert.deepEqual(await expanded(page), ['true', 'false', 'true']);
    assert.equal(await page.locator('.session-turn-pending').innerText(), 'Agent 正在处理，尚未生成最终结论。');
    await page.getByRole('button', { name: '最新一轮', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('.project-aow-snapshot-navigation li:last-child button')?.getAttribute('aria-current') === 'location');
    const finished = structuredClone(next);
    finished.status = 'completed';
    finished.turns.at(-1).status = 'completed';
    finished.turns.at(-1).final = { text: '本轮执行结束。', timestamp: null };
    await update(page, { snapshot: finished });
    await page.getByText('本轮执行结束。', { exact: true }).waitFor();
    assert.deepEqual(await expanded(page), ['true', 'false', 'false']);
    await toggles(page).last().click();
    await page.getByRole('button', { name: '刷新会话快照' }).click();
    assert.deepEqual(await expanded(page), ['true', 'false', 'true']);
    await update(page, { session: { ...session, id: 'another-session' }, snapshot });
    await page.waitForFunction(() => document.querySelectorAll('.session-process-toggle').length === 2);
    assert.deepEqual(await expanded(page), ['false', 'false']);
  });

  await test('legacy, loading, empty, error and interrupted snapshots stay readable', async (t) => {
    const { page } = await open(t);
    await update(page, { snapshot: undefined, loading: true });
    await page.getByText('正在读取会话快照…').waitFor();
    await update(page, { loading: false, error: '读取失败，请重试' });
    await page.getByRole('button', { name: '重试', exact: true }).waitFor();
    await update(page, { error: undefined, snapshot: { ...snapshot, turns: [] } });
    await page.getByText('这个会话还没有可展示的用户输入。').waitFor();
    await update(page, { snapshot: { ...snapshot, turns: [makeTurn('legacy', '旧记录', '旧结论', [])] } });
    await page.getByText('旧结论', { exact: true }).waitFor();
    assert.equal(await toggles(page).count(), 0);
    await update(page, { snapshot: { ...snapshot, turns: [makeTurn('stopped', '停止运行', null, [{ kind: 'tool', text: 'exec_command', status: 'interrupted' }], 'interrupted')] } });
    await page.getByText('本轮已中断，未生成最终结论。').waitFor();
    assert.deepEqual(await expanded(page), ['false']);
  });

  await test('desktop narrow panes contain long names, Markdown tables and tool failures', async (t) => {
    const { page } = await open(t, false, 420);
    const long = 'very_long_tool_name_'.repeat(14);
    await update(page, { snapshot: { ...snapshot, turns: [makeTurn('long', '窄屏下的长内容', '| 内容 |\n| --- |\n| ' + 'long-content'.repeat(30) + ' |', [
      { kind: 'commentary', text: '<script>window.__unsafe=true</script>' },
      { kind: 'tool', text: long, status: 'failed' },
      { kind: 'tool', text: 'Read', status: 'unknown' },
    ])] } });
    await toggles(page).last().click();
    await page.locator('.session-tool-group-toggle').click();
    await page.locator('.session-tool-group-details').getByText('结果未记录', { exact: true }).waitFor();
    assert.equal(await page.locator('.project-aow-snapshot-navigation').isVisible(), false);
    assert.equal(await page.evaluate(() => window.__unsafe), undefined);
    await noOverflow(page);
  });

  await test('mobile shares collapse rules and follows newly refreshed rounds without overflow', async (t) => {
    const { page, updateSnapshot } = await open(t, true);
    assert.deepEqual(await expanded(page), ['false', 'false']);
    await toggles(page).first().click();
    await page.getByRole('button', { name: '刷新', exact: true }).click();
    assert.deepEqual(await expanded(page), ['true', 'false']);
    const next = { ...snapshot, status: 'in_progress', turns: [...snapshot.turns, makeTurn('mobile-new', '新一轮', null, [{ kind: 'tool', text: 'exec_command', status: 'in_progress' }], 'in_progress')] };
    updateSnapshot(next);
    await page.getByRole('button', { name: '刷新', exact: true }).click();
    await page.locator('.mobile-turn').nth(2).waitFor();
    assert.deepEqual(await expanded(page), ['true', 'false', 'true']);
    await page.setViewportSize({ width: 320, height: 740 });
    await noOverflow(page);
    const failed = structuredClone(next);
    failed.status = 'failed';
    failed.turns.at(-1).status = 'failed';
    updateSnapshot(failed);
    await page.getByRole('button', { name: '刷新', exact: true }).click();
    await page.getByText('本轮执行失败，未生成最终结论。').waitFor();
    assert.deepEqual(await expanded(page), ['true', 'false', 'false']);
  });

  await test('adjacent tools collapse into English summaries with failures visible and details available', async (t) => {
    const { page } = await open(t);
    const grouped = { ...snapshot, turns: [makeTurn('grouped', '验证连续工具合并', '最终结论始终可见', [
      { kind: 'tool', text: 'exec', status: 'completed' },
      { kind: 'tool', text: 'tool_search', status: 'completed' },
      { kind: 'tool', text: 'exec_command', actions: ['read_files'], status: 'completed' },
      { kind: 'tool', text: 'apply_patch', status: 'completed' },
      { kind: 'tool', text: 'exec_command', status: 'failed' },
      { kind: 'commentary', text: '已定位失败原因，继续验证。' },
      { kind: 'tool', text: 'exec_command', status: 'completed' },
      { kind: 'tool', text: 'Read', status: 'completed' },
    ])] };
    await update(page, { snapshot: grouped });
    await toggles(page).last().click();
    const groups = page.locator('.session-tool-group-toggle');
    await groups.nth(1).waitFor();
    assert.equal(await groups.count(), 2, 'commentary separates tool groups');
    assert.equal(await groups.first().getAttribute('aria-expanded'), 'false');
    assert.equal(await page.locator('.session-tool-group-details').count(), 0);
    assert.match(await groups.first().innerText(), /Loaded tools, read files, edited files, ran commands/);
    assert.match(await groups.first().innerText(), /5 次调用/);
    assert.match(await groups.first().innerText(), /1 次失败/);
    await groups.first().focus();
    await page.keyboard.press('Enter');
    assert.equal(await page.locator('.session-tool-group-details > li').count(), 5);
    await page.getByRole('button', { name: '刷新会话快照' }).click();
    assert.equal(await groups.first().getAttribute('aria-expanded'), 'true');
    const updated = structuredClone(grouped);
    updated.turns[0].activities.push({ id: 'appended', kind: 'tool', text: 'Read', status: 'in_progress' });
    await update(page, { snapshot: updated });
    await groups.last().getByText('1 次执行中', { exact: true }).waitFor();
    assert.equal(await groups.first().getAttribute('aria-expanded'), 'true');
    assert.equal(await groups.last().getAttribute('aria-expanded'), 'false');
    await page.setViewportSize({ width: 320, height: 740 });
    await noOverflow(page);
    await groups.first().click();
    assert.equal(await page.locator('.session-tool-group-details').count(), 0);
    assert.equal(await page.getByText('最终结论始终可见', { exact: true }).isVisible(), true);
  });

  for (const mobile of [false, true]) {
    await test(`${mobile ? 'mobile' : 'desktop'} flowcharts survive snapshot refreshes and content updates`, async (t) => {
      const { page, updateSnapshot } = await open(t, mobile);
      const diagram = [
        '1. 共享刷新', '',
        '   ```mermaid',
        '   flowchart LR',
        '       A[共享文件监听服务] --> B[Explorer 按需刷新]',
        '       A --> C[Git 防抖与限流]',
        '       C --> D[git status]',
        '       D --> E[Changes 列表]',
        '   ```',
      ].join('\n');
      let next = { ...snapshot, turns: [makeTurn('diagram', '查看流程图', diagram, [])] };
      async function show() {
        if (mobile) {
          updateSnapshot(next);
          const response = page.waitForResponse((response) => new URL(response.url()).pathname.endsWith('/snapshot'));
          await page.getByRole('button', { name: '刷新', exact: true }).click();
          await (await response).finished();
          await page.locator('button[aria-label="刷新"]:enabled').waitFor();
        } else await update(page, { snapshot: next });
      }
      await show();
      const svg = page.locator('.project-aow-snapshot-flowchart svg');
      await svg.waitFor();
      assert.match((await svg.locator('g.node').allTextContents()).join(''), /共享文件监听服务/);
      const id = await svg.getAttribute('id');
      next = { ...next, captured_at: '2026-09-18T02:00:00Z' };
      await show();
      assert.equal(await svg.count(), 1, 'refresh must not restore the Mermaid source block');
      assert.equal(await svg.getAttribute('id'), id, 'unchanged content keeps the rendered diagram');
      next = { ...next, turns: [makeTurn('diagram', '查看流程图', diagram.replace('```mermaid', '```flowchart').replace('Changes 列表', '更新后的列表'), [])] };
      await show();
      await page.waitForFunction((id) => {
        const diagram = document.querySelector('.project-aow-snapshot-flowchart svg');
        return diagram && diagram.id !== id;
      }, id);
      assert.notEqual(await svg.getAttribute('id'), id, 'changed content renders a new diagram');
      assert.match((await svg.locator('g.node').allTextContents()).join(''), /更新后的列表/);
      assert.equal(await page.locator('.project-aow-snapshot-flowchart-error').count(), 0);
      await noOverflow(page);
      if (process.env.SESSION_TEST_SCREENSHOTS) {
        await mkdir(process.env.SESSION_TEST_SCREENSHOTS, { recursive: true });
        await page.screenshot({ path: `${process.env.SESSION_TEST_SCREENSHOTS}/flowchart-${mobile ? 'mobile' : 'desktop'}.png` });
      }
      next = { ...next, turns: [makeTurn('diagram', '查看流程图', '普通文本\n\n```text\nflowchart LR\nA --> B\n```', [])] };
      await show();
      await page.locator('code.language-text').waitFor();
      assert.equal(await svg.count(), 0);
    });

    await test(`${mobile ? 'mobile' : 'desktop'} tool details show commands and failures, preserve choices and contain long output`, async (t) => {
      const { page, updateSnapshot } = await open(t, mobile);
      const value = (text, truncated = false) => ({ text, truncated });
      const next = { ...snapshot, turns: [makeTurn('details', '检查命令详情', '检查结束', [
        { kind: 'tool', text: 'exec_command', status: 'completed', details: {
          command: value('rg -n needle src'), cwd: value('/workspace/aow'),
          input: value('{"cmd":"rg -n needle src","workdir":"/workspace/aow"}'),
          output: value('src/main.rs:10:needle'), exit_code: 0,
        } },
        { kind: 'tool', text: 'exec_command', status: 'failed', details: {
          command: value('npm test'), cwd: value('/workspace/aow'), exit_code: 1, duration_ms: 1250,
          error: value('error: missing configuration'),
          output: value('<img src=x onerror="window.__unsafe=true">\n' + 'long_output_'.repeat(200) + '\nfinal failure', true),
        } },
        { kind: 'tool', text: 'search_files', status: 'completed', details: {
          input: value('{"query":"needle","path":"src"}'), output: value('src/main.rs'),
        } },
      ])] };
      async function show(value) {
        if (mobile) {
          updateSnapshot(value);
          await page.getByRole('button', { name: '刷新', exact: true }).click();
        } else await update(page, { snapshot: value });
      }
      await show(next);
      await page.getByText('检查结束', { exact: true }).waitFor();
      await toggles(page).last().click();
      const group = page.locator('.session-tool-group-toggle');
      await group.waitFor();
      assert.equal(await group.getAttribute('aria-expanded'), 'false');
      await group.click();
      const rows = page.locator('.session-process-tool.has-details');
      const success = rows.nth(0);
      const failed = rows.nth(1);
      assert.equal(await success.locator('.session-tool-toggle').getAttribute('aria-expanded'), 'false');
      assert.equal(await success.locator('.session-tool-preview').innerText(), 'rg -n needle src');
      assert.equal(await failed.locator('.session-tool-toggle').getAttribute('aria-expanded'), 'false');
      await failed.locator('.session-tool-toggle').click();
      await page.getByText('error: missing configuration', { exact: true }).waitFor();
      assert.match(await failed.innerText(), /退出码：1/);
      assert.match(await failed.innerText(), /耗时：1250 ms/);
      assert.match(await failed.innerText(), /内容过长，仅保留开头和结尾/);
      assert.equal(await failed.locator('pre img').count(), 0);
      assert.equal(await page.evaluate(() => window.__unsafe), undefined);

      await success.locator('.session-tool-toggle').focus();
      await page.keyboard.press('Enter');
      await success.getByText('src/main.rs:10:needle', { exact: true }).waitFor();
      await success.locator('.session-tool-input > summary').click();
      await success.getByText(next.turns[0].activities[0].details.input.text, { exact: true }).waitFor();
      await failed.locator('.session-tool-toggle').click();
      await show(structuredClone(next));
      assert.equal(await failed.locator('.session-tool-toggle').getAttribute('aria-expanded'), 'false');
      assert.equal(await success.locator('.session-tool-toggle').getAttribute('aria-expanded'), 'true');
      await failed.locator('.session-tool-toggle').click();
      await rows.nth(2).locator('.session-tool-toggle').click();
      assert.match(await rows.nth(2).locator('.session-tool-detail').innerText(), /"query":"needle"/);
      await page.setViewportSize({ width: 320, height: 740 });
      await noOverflow(page);
    });
  }

  await test('a running call stays collapsed on failure and empty output is available on demand', async (t) => {
    const { page } = await open(t);
    const next = { ...snapshot, turns: [makeTurn('running', '执行测试', null, [
      { kind: 'tool', text: 'exec_command', status: 'in_progress', details: { command: { text: 'npm test', truncated: false } } },
      { kind: 'tool', text: 'Read', status: 'completed' },
    ], 'in_progress')] };
    await update(page, { snapshot: next });
    await page.locator('.session-tool-group-toggle').click();
    const toggle = page.locator('.session-tool-toggle');
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
    const completed = structuredClone(next);
    completed.turns[0].activities[0].status = 'failed';
    completed.turns[0].activities[0].details.exit_code = 1;
    await update(page, { snapshot: completed });
    await toggle.locator('.session-process-status.failed').waitFor();
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
    await toggle.click();
    await page.getByText('未记录输出内容。', { exact: true }).waitFor();
    assert.equal(await toggle.getAttribute('aria-expanded'), 'true');
  });

  if (process.env.SESSION_TEST_SCREENSHOTS) {
    await mkdir(process.env.SESSION_TEST_SCREENSHOTS, { recursive: true });
    for (const [name, width, mobile] of [['desktop', 1280, false], ['narrow', 540, false], ['mobile', 390, true]]) {
      const page = await browser.newPage({ viewport: { width, height: 960 } });
      await page.route('**/api/**', (route) => route.fulfill({ json: snapshot }));
      await page.goto(`${baseURL}${mobile ? '?mobile' : ''}`);
      await toggles(page).first().waitFor();
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: `${process.env.SESSION_TEST_SCREENSHOTS}/${name}.png` });
      await page.close();
    }
  }
} finally {
  await browser?.close();
  await server.close();
}
