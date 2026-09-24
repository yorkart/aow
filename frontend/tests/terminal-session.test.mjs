import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
const cwd = '/workspace/demo';
const session = (id = 'session-one', title = '修复终端会话', agent = 'codex') => ({ id: `${agent}:${id}`, session_id: id, agent, title, cwd, created_at: '2026-09-17T00:00:00Z', updated_at: '2026-09-17T01:00:00Z' });
const data = (sessions = [session()]) => ({ agent: 'codex', cwd, title: '⠋ 修复终端会话 | demo', process: { pid: 123, start_time: '1000', cwd }, live_session_id: null, sessions });

try {
  await server.listen();
  const { terminalSessionCandidates, terminalSessionTitle } = await server.ssrLoadModule('/src/features/terminals/terminalSessionMatching.ts');
  await test('normalizes activity but never guesses from a project name, partial title or other directory', () => {
    assert.equal(terminalSessionTitle('[ ! ] Action Required | 修复终端会话 ⠙ | demo', cwd), '修复终端会话');
    assert.equal(terminalSessionTitle('[ . ] Action Required | 修复终端会话 | demo', cwd), '修复终端会话');
    assert.equal(terminalSessionTitle('⠋ demo', cwd), '');
    assert.equal(terminalSessionCandidates(data()).automatic.session_id, 'session-one');
    assert.equal(terminalSessionCandidates(data([session(), session('two')])).automatic, undefined);
    assert.equal(terminalSessionCandidates({ ...data(), title: 'demo' }).automatic, undefined);
    assert.equal(terminalSessionCandidates({ ...data(), process: null }).automatic, undefined);
    const long = 'A long conversation title with a shared prefix';
    const partial = terminalSessionCandidates({ ...data([session('a', long), session('b', `${long} second`)]), title: 'A long conversation title... | demo' });
    assert.equal(partial.matches.length, 2);
    assert.equal(partial.automatic, undefined);
    assert.equal(terminalSessionCandidates(data([{ ...session(), cwd: `${cwd}/child` }])).automatic, undefined);
    assert.equal(terminalSessionCandidates({ ...data(), live_session_id: 'not-yet-written' }).automatic, undefined);
    assert.equal(terminalSessionCandidates({ ...data([session('trae', '修复终端会话', 'traecli')]), agent: 'traecli' }).automatic.session_id, 'trae');
  });
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const url = `http://127.0.0.1:${server.httpServer.address().port}/tests/terminal-session-preview.html`;

  async function open(t, initial = data()) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    page.setDefaultTimeout(10_000);
    t.after(() => page.close());
    const errors = [], mutations = [], sockets = [], inputs = [], snapshots = [];
    page.on('pageerror', error => errors.push(error.message));
    t.after(() => { assert.deepEqual(errors, []); assert.deepEqual(mutations, []); });
    let response = initial;
    let fail = false;
    let nextReply;
    await page.route('**/api/**', async route => {
      const request = route.request();
      const path = new URL(request.url()).pathname;
      if (request.method() !== 'GET') mutations.push(path);
      if (path.endsWith('/agent-sessions')) {
        const reply = fail ? { status: 503, json: { message: '查询暂时失败' } } : { json: response };
        if (nextReply) { const wait = nextReply; nextReply = undefined; await wait(); }
        await route.fulfill(reply).catch(() => {});
      } else if (path.endsWith('/snapshot')) {
        const id = decodeURIComponent(path.split('/').at(-2));
        snapshots.push(id);
        await route.fulfill({ json: { session_id: id, agent: response.agent, cwd, captured_at: '2026-09-17T01:00:00Z', status: 'in_progress', truncated: false, turns: [] } });
      } else await route.fulfill({ status: 404, json: { message: 'No test fixture' } });
    });
    await page.routeWebSocket('**/api/terminals/**/ws?**', socket => {
      sockets.push(socket);
      socket.onMessage(message => {
        if (typeof message !== 'string') { inputs.push(message); return; }
        const control = JSON.parse(message);
        if (control.type === 'claim') {
          socket.send(JSON.stringify({ type: 'control', state: 'claimed' }));
          socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch', offset: 0, reset: true, replay_bytes: 0,
            restore_cols: 80, restore_rows: 24, restore: '\x1b[2J\x1b[HKEEP TERMINAL ALIVE\r\n$ ' }));
        }
      });
    });
    await page.goto(url);
    await page.locator('.terminal-emulator-shell:not(.restore-pending)').first().waitFor();
    const pane = page.locator('.terminal-pane').first();
    return { page, pane, sockets, inputs, snapshots, setData(value) { response = value; }, fail(value) { fail = value; },
      delayNext() {
        let release, started;
        const waiting = new Promise(resolve => { release = resolve; });
        const entered = new Promise(resolve => { started = resolve; });
        nextReply = () => { started(); return waiting; };
        return { release, entered };
      },
    };
  }
  async function update(page, patch) { await page.evaluate(patch => window.terminalSessionPreview.update(patch), patch); }

  await test('same-pane details retain the terminal DOM, connection, OSC title and other split', async t => {
    const state = await open(t);
    const { page, pane, sockets, inputs, snapshots } = state;
    await pane.locator('.xterm').evaluate(node => { window.originalXterm = node; });
    const socketCount = sockets.length;
    await pane.getByRole('button', { name: '切换到会话详情' }).click();
    await pane.getByRole('heading', { name: '修复终端会话' }).waitFor();
    assert.equal(await pane.locator('.terminal-pane-name').innerText(), '⠋ 修复终端会话 | demo');
    assert.equal(await page.locator('.terminal-pane').nth(1).locator('.terminal-pane-surface').isVisible(), true);
    await page.screenshot({ path: '/tmp/aow-terminal-session.png' });
    state.setData(data([session(), session('namesake')]));
    await pane.getByRole('button', { name: '刷新会话列表' }).click();
    await pane.locator('.terminal-agent-session-list > button').nth(1).waitFor();
    assert.equal(await pane.locator('.project-aow-session-snapshot').count(), 0);
    await pane.locator('.terminal-agent-session-list > button').first().click();
    await pane.getByRole('button', { name: '返回终端' }).click();
    assert.equal(await pane.locator('.xterm').evaluate(node => node === window.originalXterm), true);
    assert.equal(sockets.length, socketCount);
    assert.deepEqual(inputs, []);
    assert.ok(snapshots.includes('session-one'));
    await pane.locator('.xterm-helper-textarea').focus();
    await page.keyboard.type('echo alive');
    assert.ok(inputs.length > 0, 'terminal must accept input after returning');
  });

  await test('ambiguous names require selection, survive activity and reopen, and reset on a new process', async t => {
    const state = await open(t, data([session(), session('session-two')]));
    const { pane, page } = state;
    await pane.getByRole('button', { name: '切换到会话详情' }).click();
    await pane.locator('.terminal-agent-session-list > button').nth(1).click();
    await pane.getByRole('heading', { name: '修复终端会话' }).waitFor();
    assert.match(await pane.locator('.terminal-agent-session-toolbar').innerText(), /已选择 · session-two/);
    await update(page, { terminalTitles: { one: '⠙ 修复终端会话 | demo' } });
    await pane.getByRole('button', { name: '返回终端' }).click();
    await pane.getByRole('button', { name: '切换到会话详情' }).click();
    await pane.getByRole('heading', { name: '修复终端会话' }).waitFor();
    assert.match(await pane.locator('.terminal-agent-session-toolbar').innerText(), /session-two/);
    await page.reload();
    await pane.getByRole('button', { name: '切换到会话详情' }).click();
    await pane.getByRole('heading', { name: '修复终端会话' }).waitFor();
    assert.match(await pane.locator('.terminal-agent-session-toolbar').innerText(), /session-two/);
    const next = { ...data([session(), session('session-two')]), process: { pid: 123, start_time: '2000', cwd } };
    state.setData(next);
    await update(page, { agentProcesses: { one: next.process } });
    await pane.locator('.terminal-agent-session-list > button').nth(1).waitFor();
    assert.equal(await pane.locator('.project-aow-session-snapshot').count(), 0);
  });

  await test('Claude PID identity wins over title and an absent transcript does not open a namesake', async t => {
    const claude = { ...data([session('first', '修复终端会话', 'claude'), session('second', '不同标题', 'claude')]), agent: 'claude', live_session_id: 'second' };
    const state = await open(t, claude);
    const { page, pane } = state;
    await update(page, { detectedAgents: { one: 'claude', two: null } });
    await pane.getByRole('button', { name: '切换到会话详情' }).click();
    await pane.getByRole('heading', { name: '不同标题' }).waitFor();
    state.setData({ ...claude, live_session_id: 'pending' });
    await pane.getByRole('button', { name: '刷新会话列表' }).click();
    await pane.getByText('已识别当前会话，记录尚未就绪。可刷新重试或手动选择。').waitFor();
    assert.equal(await pane.locator('.project-aow-session-snapshot').count(), 0);
  });

  await test('no title match and query failures allow refresh, manual choice and reselect', async t => {
    const state = await open(t, { ...data([session('another', '另一段对话')]), title: 'demo' });
    const { page, pane } = state;
    await update(page, { terminalTitles: { one: 'demo' } });
    state.fail(true);
    await pane.getByRole('button', { name: '切换到会话详情' }).click();
    await pane.getByRole('alert').waitFor();
    state.fail(false);
    await pane.getByRole('button', { name: '刷新会话列表' }).click();
    await pane.locator('.terminal-agent-session-list > button').click();
    await pane.getByRole('heading', { name: '另一段对话' }).waitFor();
    await pane.getByRole('button', { name: '重新选择' }).click();
    await pane.locator('.terminal-agent-session-list > button').waitFor();
    assert.equal(await pane.locator('.project-aow-session-snapshot').count(), 0);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  });

  await test('a delayed result from an old process cannot overwrite the current conversation', async t => {
    const state = await open(t);
    const delayed = state.delayNext();
    await state.pane.getByRole('button', { name: '切换到会话详情' }).click();
    await delayed.entered;
    const next = { ...data([session('new', '新的对话')]), title: '新的对话 | demo', process: { pid: 456, start_time: '2000', cwd } };
    state.setData(next);
    await update(state.page, { agentProcesses: { one: next.process }, terminalTitles: { one: next.title } });
    await state.pane.getByRole('heading', { name: '新的对话' }).waitFor();
    delayed.release();
    assert.match(await state.pane.locator('.terminal-agent-session-toolbar').innerText(), /new/);
    assert.equal(state.snapshots.includes('session-one'), false);
  });
} finally {
  await browser?.close();
  await server.close();
}
