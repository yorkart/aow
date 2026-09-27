import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const common = { revision: 1, agent: 'codex', project_id: 'project', project_name: 'Project', workspace_mode: 'existing', workspace_path: '/repo', cleanup_worktree: false,
    base_branch: 'main', interval_seconds: null, max_concurrent_runs: 1, enabled: true, yolo: true, precheck_command: '', precheck_timeout_seconds: 60, failure_notification: null,
    created_at: '2026-09-23T00:00:00Z', updated_at: '2026-09-23T00:00:00Z', scheduler_error: null, next_run_at: null, last_run: null, is_running: false };
  async function fixture(t, { history = [] } = {}) {
    const page = await browser.newPage({ viewport: { width: 1400, height: 1000 } });
    page.setDefaultTimeout(15000);
    t.after(() => page.close());
    const errors = [], runs = [], saves = [];
    page.on('pageerror', error => errors.push(error.message));
    const prompt = '检查 🧪 {{分支}} / {{分支}} / {{保持原文}}';
    const bindings = [...prompt.matchAll(/\{\{分支\}\}/gu)].map(match => ({ name: '分支', placeholder: match[0], start: Buffer.byteLength(prompt.slice(0, match.index)), end: Buffer.byteLength(prompt.slice(0, match.index + match[0].length)) }));
    const tasks = [
      { ...common, id: '12345678', name: '每日检查', kind: 'scheduled', cron: '0 9 * * *', prompt: '检查代码', prompt_bindings: [] },
      { ...common, id: '87654321', name: '分支检查', kind: 'manual', cron: '', prompt, prompt_bindings: bindings },
      { ...common, id: '11223344', name: '直接执行', kind: 'manual', cron: '', prompt: '不需要输入', prompt_bindings: [] },
    ];
    await page.route('**/api/aow/notification-settings', route => route.fulfill({ json: { im: { providers: [] } } }));
    await page.route('**/api/aow/automations**', route => {
      const url = new URL(route.request().url());
      const suffix = url.pathname.split('/automations')[1];
      if (suffix === '/status') return route.fulfill({ json: { platform: 'systemd', ready: true, timezone: 'UTC' } });
      if (suffix.endsWith('/run')) { runs.push({ id: suffix.split('/')[1], ...route.request().postDataJSON() }); return route.fulfill({ json: { run_id: 'run-123' } }); }
      if (suffix.endsWith('/runs')) return route.fulfill({ json: history });
      if (suffix.includes('/runs/')) return route.fulfill({ json: history.find(run => run.id === suffix.split('/').at(-1)) });
      if (route.request().method() === 'POST' || route.request().method() === 'PUT') {
        const input = route.request().postDataJSON(); saves.push(input);
        const task = { ...common, id: suffix.slice(1) || '99887766', ...input };
        const index = tasks.findIndex(item => item.id === task.id);
        if (index >= 0) tasks[index] = task; else tasks.push(task);
        return route.fulfill({ json: task });
      }
      return route.fulfill({ json: suffix ? tasks.find(task => task.id === suffix.slice(1)) : tasks });
    });
    await page.goto(`${base}/tests/manual-tasks-preview.html`);
    await page.getByTitle('分支检查 · ID: 87654321', { exact: true }).waitFor();
    return { page, errors, runs, saves, tasks };
  }

  await test('panel splits Schedule above Manual, collapses each group, and opens the shared detail', async t => {
    const { page, errors } = await fixture(t);
    const automation = page.getByRole('region', { name: 'Schedule', exact: true });
    const manual = page.getByRole('region', { name: 'Manual', exact: true });
    assert.equal(await automation.getByRole('listitem').count(), 1);
    assert.equal(await manual.getByRole('listitem').count(), 2);
    assert.ok((await manual.boundingBox()).y > (await automation.boundingBox()).y);
    const scheduleToggle = automation.getByRole('button', { name: /^(展开|收起) Schedule$/ });
    const manualToggle = manual.getByRole('button', { name: /^(展开|收起) Manual$/ });
    const originalManualHeight = (await manual.boundingBox()).height;
    await scheduleToggle.click();
    assert.equal(await scheduleToggle.getAttribute('aria-expanded'), 'false');
    assert.equal(await automation.getByRole('list').count(), 0);
    assert.equal(await manual.getByRole('listitem').count(), 2);
    assert.equal((await manual.boundingBox()).height, originalManualHeight);
    await manualToggle.click();
    assert.equal(await manualToggle.getAttribute('aria-expanded'), 'false');
    assert.equal(await manual.getByRole('list').count(), 0);
    await manual.getByRole('button', { name: '创建手动任务', exact: true }).click();
    await page.getByRole('dialog', { name: '创建手动任务' }).getByRole('button', { name: '取消' }).click();
    assert.equal(await manualToggle.getAttribute('aria-expanded'), 'false');
    await scheduleToggle.focus();
    await page.keyboard.press('Enter');
    assert.equal(await scheduleToggle.getAttribute('aria-expanded'), 'true');
    assert.equal(await automation.getByRole('listitem').count(), 1);
    assert.equal(await manual.getByRole('list').count(), 0);
    await manualToggle.click();
    assert.equal(await manual.getByRole('listitem').count(), 2);
    await manual.getByTitle('分支检查 · ID: 87654321', { exact: true }).click();
    await page.getByRole('heading', { name: '分支检查', exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '暂停', exact: true }).count(), 0);
    assert.equal(await page.getByText('下次运行', { exact: true }).count(), 0);
    assert.equal(await page.getByRole('region', { name: '任务变量' }).locator('code').allTextContents().then(JSON.stringify), '["分支"]');
    await page.getByRole('button', { name: '执行历史', exact: true }).click();
    await page.getByText('暂无执行记录', { exact: false }).waitFor();
    assert.deepEqual(errors, []);
    await page.getByRole('button', { name: '概述', exact: true }).click();
    if (process.env.MANUAL_SCREENSHOT) await page.screenshot({ path: process.env.MANUAL_SCREENSHOT });
  });

  await test('manual run uses only saved variables, requires values, clears on each opening, and passes revision', async t => {
    const { page, runs, errors } = await fixture(t);
    await page.getByTitle('分支检查 · ID: 87654321', { exact: true }).click();
    await page.getByRole('button', { name: '立即运行', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '运行手动任务' });
    assert.equal(await dialog.getByRole('textbox').count(), 1);
    assert.equal(await dialog.getByRole('button', { name: '运行', exact: true }).isDisabled(), true);
    await dialog.getByLabel('分支', { exact: true }).fill('   ');
    assert.equal(await dialog.getByRole('button', { name: '运行', exact: true }).isDisabled(), true);
    await dialog.getByLabel('分支', { exact: true }).fill('feat/中文\n{{分支}} $1');
    await dialog.getByRole('button', { name: '运行', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    assert.deepEqual(runs[0], { id: '87654321', revision: 1, variables: { 分支: 'feat/中文\n{{分支}} $1' } });
    await page.getByRole('button', { name: '立即运行', exact: true }).click();
    assert.equal(await dialog.getByLabel('分支', { exact: true }).inputValue(), '');
    await dialog.getByRole('button', { name: '取消' }).click();
    assert.equal(runs.length, 1);
    await page.getByTitle('直接执行 · ID: 11223344', { exact: true }).click();
    await page.getByRole('heading', { name: '直接执行', exact: true }).waitFor();
    await page.getByRole('button', { name: '立即运行', exact: true }).click();
    await page.getByRole('status').waitFor();
    assert.equal(runs.length, 2);
    assert.deepEqual(runs[1].variables, {});
    assert.deepEqual(errors, []);
  });

  await test('editor preserves existing bindings until prompt changes and displays the saved replacements', async t => {
    const { page, saves, errors } = await fixture(t);
    await page.getByTitle('分支检查 · ID: 87654321', { exact: true }).click();
    await page.getByRole('button', { name: '编辑', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '编辑手动任务' });
    await dialog.locator('.monaco-editor').waitFor();
    assert.deepEqual(await dialog.getByRole('region', { name: '任务变量' }).locator('code').allTextContents(), ['分支']);
    assert.equal(await dialog.getByLabel('运行计划', { exact: true }).count(), 0);
    assert.equal(await dialog.getByLabel('启用自动化', { exact: true }).count(), 0);
    await dialog.getByLabel('名称', { exact: true }).fill('重新命名');
    await dialog.getByRole('button', { name: '保存更改' }).click();
    await dialog.waitFor({ state: 'hidden' });
    assert.equal(saves[0].prompt_bindings.length, 2);
    await page.getByRole('button', { name: '编辑', exact: true }).click();
    await dialog.locator('.monaco-editor textarea').focus();
    await page.keyboard.press('ControlOrMeta+a');
    await page.keyboard.insertText('执行 🧪 {{分支}} 和 {{ 检查重点 }}，再次 {{分支}}');
    await dialog.getByRole('region', { name: '任务变量' }).getByText('检查重点', { exact: true }).waitFor();
    assert.deepEqual(await dialog.getByRole('region', { name: '任务变量' }).locator('code').allTextContents(), ['分支', '检查重点']);
    await dialog.getByRole('button', { name: '保存更改' }).click();
    await dialog.waitFor({ state: 'hidden' });
    const saved = saves[1];
    assert.equal(saved.kind, 'manual'); assert.equal(saved.cron, ''); assert.equal(saved.interval_seconds, null);
    assert.equal(saved.prompt_bindings.length, 3);
    for (const binding of saved.prompt_bindings) assert.equal(Buffer.from(saved.prompt).subarray(binding.start, binding.end).toString(), binding.placeholder);
    assert.deepEqual(errors, []);
  });

  await test('execution details show captured parameters per run, preserve literal text, and distinguish legacy and empty inputs', async t => {
    const run = { task_id: '87654321', task_revision: 1, task_name: '分支检查', agent: 'codex', source: 'manual', status: 'completed', started_at: '2026-09-23T01:00:00Z', finished_at: '2026-09-23T01:00:01Z', workspace_path: '/repo', branch: null, session_id: null, agent_pid: null, agent_command: null, exit_code: 0, message: null, preparation_ms: 0, session_acquired_ms: null, duration_ms: 1000 };
    const literal = 'feature/中文\n{{pr}} $1 <script>text</script>' + '/very-long-branch-name'.repeat(15);
    const history = [
      { ...run, id: '05', variables: { pr: literal, mode: 'review' } },
      { ...run, id: '04', status: 'failed', variables: { pr: 'another-branch', mode: 'review' } },
      { ...run, id: '03' },
      { ...run, id: '02', variables: {} },
      { ...run, id: '01', source: 'scheduled' },
    ];
    const { page, errors } = await fixture(t, { history });
    await page.getByTitle('分支检查 · ID: 87654321', { exact: true }).click();
    await page.getByRole('button', { name: '执行历史', exact: true }).click();
    await page.locator('.automation-run-summary').first().waitFor();
    assert.equal(await page.locator('.automation-run-summary').count(), 5);
    await page.locator('.automation-run-heading').getByText('参数', { exact: true }).waitFor();
    const summaries = page.locator('.automation-parameter-summary');
    assert.equal(await summaries.nth(1).textContent(), 'pr=another-branch,mode=review');
    assert.equal(await summaries.nth(2).textContent(), '未记录');
    assert.equal(await summaries.nth(3).textContent(), '—');
    const summary = await summaries.first().textContent();
    assert.ok(summary.endsWith('…') && Array.from(summary).length <= 101);
    assert.equal(summary.includes('\n'), false);
    await summaries.first().hover();
    const tooltip = page.getByRole('tooltip', { name: '执行参数' });
    await tooltip.waitFor();
    assert.deepEqual(await tooltip.locator('code').allTextContents(), ['pr', literal, 'mode', 'review']);
    assert.equal(await tooltip.locator('script').count(), 0);
    assert.equal(await page.locator('.automation-run-detail').count(), 0);
    await tooltip.hover();
    await tooltip.locator('code').first().click();
    assert.equal(await page.getByRole('region', { name: '触发参数', exact: true }).count(), 0);
    assert.equal(await tooltip.isVisible(), true);
    if (process.env.PARAMETERS_SCREENSHOT) await page.screenshot({ path: process.env.PARAMETERS_SCREENSHOT });
    await page.keyboard.press('Escape');
    await tooltip.waitFor({ state: 'hidden' });
    for (const width of [800, 640]) {
      await page.setViewportSize({ width, height: 700 });
      await page.mouse.move(0, 0);
      await summaries.first().hover();
      await tooltip.waitFor();
      const bounds = await tooltip.boundingBox();
      assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= width && bounds.y >= 0 && bounds.y + bounds.height <= 700);
      assert.ok(await summaries.first().evaluate(node => node.clientWidth > 0));
      assert.ok(await page.locator('.automation-run-summary').first().evaluate(node => node.scrollWidth <= node.clientWidth));
      await page.keyboard.press('Escape');
    }
    await page.setViewportSize({ width: 1400, height: 1000 });
    const parameters = page.getByRole('region', { name: '触发参数', exact: true });
    await page.locator('.automation-run-summary').nth(0).click();
    await parameters.waitFor();
    assert.deepEqual(await parameters.locator('dt').allTextContents(), ['pr', 'mode']);
    assert.equal(await parameters.locator('pre').first().textContent(), literal);
    assert.equal(await parameters.locator('script').count(), 0);
    await page.locator('.automation-run-summary').nth(1).click();
    assert.equal(await parameters.locator('pre').first().textContent(), 'another-branch');
    await page.locator('.automation-run-summary').nth(2).click();
    await parameters.getByText('该执行记录未保存触发参数。', { exact: true }).waitFor();
    await page.locator('.automation-run-summary').nth(3).click();
    await parameters.getByText('本次执行无参数。', { exact: true }).waitFor();
    await page.locator('.automation-run-summary').nth(4).click();
    assert.equal(await parameters.count(), 0);
    assert.deepEqual(errors, []);
  });

  await test('Manual has a separate creation entry without schedule fields', async t => {
    const { page } = await fixture(t);
    await page.getByRole('button', { name: '创建手动任务', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '创建手动任务' });
    await dialog.waitFor();
    assert.equal(await dialog.getByLabel('运行计划', { exact: true }).count(), 0);
    assert.equal(await dialog.getByRole('region', { name: '任务变量' }).count(), 1);
    await dialog.getByRole('button', { name: '取消' }).click();
  });
} finally { await browser?.close(); await server.close(); }
