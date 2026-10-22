import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadModule } from './helpers/load-module.mjs';

const runtime = new URL('../src/lib/i18n/index.ts', import.meta.url);

test('English is the default, unsupported preferences fall back to English, and blocked storage is tolerated', async () => {
  for (const preference of [null, '', 'fr-FR', 'en-US']) {
    const module = await loadModule(runtime, preference);
    assert.equal(module.getLanguage(), 'en-US');
    assert.equal(module.t('auth.sign_in_to_aow'), 'Sign in to AoW');
  }
  const blocked = await loadModule(runtime, 'zh-CN', { blockedStorage: true });
  assert.equal(blocked.getLanguage(), 'en-US');
});

test('Chinese preference and region aliases resolve to the supported catalogue', async () => {
  for (const preference of ['zh-CN', 'zh', 'zh_Hans']) {
    const module = await loadModule(runtime, preference);
    assert.equal(module.getLanguage(), 'zh-CN');
    assert.equal(module.t('auth.sign_in_to_aow'), '登录 AoW');
    assert.equal(module.resolveLanguage('zh-TW'), 'en-US');
  }
});

test('interpolation preserves untrusted text as text and never evaluates injected placeholders', async () => {
  const module = await loadModule(runtime, 'en-US');
  const text = module.t('auth.unable_to_read_login_status_http', { value0: '<script>{{value0}}</script>' });
  assert.equal(text, 'Unable to read login status (HTTP <script>{{value0}}</script>)');
});

test('locale headers preserve existing Headers and tuple-style headers', async () => {
  const module = await loadModule(runtime, 'zh-CN');
  for (const input of [new Headers({ 'Content-Type': 'application/json' }), [['Content-Type', 'application/json']]]) {
    const headers = module.languageHeaders(input);
    assert.equal(headers.get('Accept-Language'), 'zh-CN');
    assert.equal(headers.get('Content-Type'), 'application/json');
  }
});


test('English counts use singular and plural forms', async () => {
  const module = await loadModule(runtime, 'en-US');
  assert.equal(module.t('acp.call_count', { count: 1 }), '1 call');
  assert.equal(module.t('acp.call_count', { count: 2 }), '2 calls');
});

test('terminal WebSocket URLs carry the selected locale and retain resume and capability parameters', async () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, 'window');
  Object.defineProperty(globalThis, 'window', { configurable: true, value: { location: new URL('https://aow.example.com/') } });
  try {
    for (const language of ['en-US', 'zh-CN']) {
      const { terminalApi } = await loadModule(new URL('../src/features/terminals/terminalApi.ts', import.meta.url), language);
      const url = new URL(terminalApi.webSocketUrl('tab/1', 'pane/2', { epoch: 'epoch-1', after: 42 }));
      assert.equal(url.protocol, 'wss:');
      assert.equal(url.searchParams.get('lang'), language);
      assert.equal(url.searchParams.get('epoch'), 'epoch-1');
      assert.equal(url.searchParams.get('after'), '42');
      assert.equal(url.searchParams.get('control'), 'v2');
      assert.equal(url.searchParams.get('capabilities'), 'vt-snapshot-v1');
      assert.equal(url.searchParams.get('observer'), 'v1');
    }
  } finally {
    if (descriptor) Object.defineProperty(globalThis, 'window', descriptor);
    else delete globalThis.window;
  }
});

test('terminal agent counts localize while physical key names and custom tab names stay intact', async () => {
  for (const [language, expected] of [['en-US', '2 agents'], ['zh-CN', '2 个代理']]) {
    const module = await loadModule(new URL('../src/features/terminals/terminalPresentation.ts', import.meta.url), language);
    assert.deepEqual(module.terminalAccessoryKeys.slice(0, 4).map(key => key.label), ['Tab', 'Enter', 'Shift+Tab', 'Space']);
    const tab = { name: 'user {{name}}', name_is_custom: false,
      layout: { type: 'split', first: { type: 'pane', pane_id: 'one' }, second: { type: 'pane', pane_id: 'two' } },
      panes: ['one', 'two'].map(id => ({ id, cwd: '/user/project', shell: '/bin/sh', status: 'running', agent_id: 'codex' })) };
    assert.equal(module.terminalTabPresentation(tab, {}, {}).title, expected);
    assert.equal(module.terminalTabPresentation({ ...tab, name_is_custom: true }, {}, {}).title, tab.name);
  }
});
