import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const content = String.raw`# Project configuration
[package]
name = "aow"
enabled = true
count = 1_024
ratio = -1.25e+2
hex = 0xCA_FE
octal = 0o755
mask = 0b1010_0101
infinite = +inf
date = 1979-05-27T07:32:00Z
time = 07:32:00.123
dependency = { version = "1", features = ["derive"], optional = false }
"quoted.key".enabled = true
3.14159 = 'pi'
escaped = "tab\tunicode\u4E2D"
multiline = """first # still a string
key = \"still text\"
last"""
literal = '''C:\temp # still a string
name = "not a key"'''
after = false # real comment
nested = [
  [1, 2],
  [3, 4],
]
[[bin]]
name = 'aow-cli'
[target.'cfg(unix)'.dependencies]
serde = "1"
`;
const javascript = '// Module entry\nexport const message = "hello";\nexport const count = 1_024;\n';
const jsonc = '// Configuration\n{\n  /* Keep this comment */\n  "name": "aow",\n  "enabled": true,\n  "count": 1024,\n  "items": [1, 2,],\n}\n';
const harness = `<!doctype html><html><head><meta charset="utf-8"></head><body><div id="root"></div><script type="module">
import React from 'react';
import { createRoot } from 'react-dom/client';
import MonacoEditor, { DiffEditor } from '/src/features/editor/MonacoEditor.tsx';
import { monaco } from '/src/features/editor/monaco.ts';
import { editorLanguage } from '/src/features/editor/language.ts';
window.monaco = monaco;
const params = new URLSearchParams(location.search);
const filename = params.get('file') ?? 'Cargo.toml';
const language = editorLanguage(filename);
const content = language === 'toml' ? ${JSON.stringify(content)}
  : language === 'json' ? ${JSON.stringify(jsonc)} : ${JSON.stringify(javascript)};
const diff = params.has('diff');
createRoot(document.getElementById('root')).render(diff
  ? React.createElement(DiffEditor, { height: 680, language, theme: 'vs-dark', original: content, modified: content.replace('1_024', '2_048'),
      originalModelPath: 'inmemory://git-diff/test/original/' + filename, modifiedModelPath: 'inmemory://git-diff/test/modified/' + filename,
      onMount: editor => { window.diffEditor = editor; window.editor = editor.getModifiedEditor(); } })
  : React.createElement(MonacoEditor, { height: 680, path: 'inmemory://aow/test/1/' + filename, language, theme: 'vs-dark', value: content,
      onMount: editor => { window.editor = editor; } }));
</script></body></html>`;
const server = await createServer({
  root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } },
  plugins: [{ name: 'editor-languages-fixture', configureServer(server) {
    server.middlewares.use('/editor-languages-test', async (_request, response, next) => {
      try {
        response.setHeader('Content-Type', 'text/html');
        response.end(await server.transformIndexHtml('/editor-languages-test', harness));
      } catch (error) { next(error); }
    });
  } }],
});
let browser;

async function fixture(t, { diff = false, filename = 'Cargo.toml', language = 'toml' } = {}) {
  const context = await browser.newContext({ viewport: { width: 1200, height: 800 } });
  const page = await context.newPage(), errors = [];
  page.on('pageerror', error => errors.push(error.message));
  t.after(async () => { await context.close(); assert.deepEqual(errors, []); });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/editor-languages-test?file=${encodeURIComponent(filename)}${diff ? '&diff' : ''}`);
  await page.waitForFunction(language => window.editor?.getModel()?.getLanguageId() === language
    && new Set([...document.querySelectorAll('.view-lines span[class]')].map(span => getComputedStyle(span).color)).size >= 3, language);
  return page;
}

function tokenType(tokens, line, fragment) {
  const index = content.split('\n').indexOf(line);
  assert.ok(index >= 0, line);
  const offset = line.indexOf(fragment);
  assert.ok(offset >= 0, fragment);
  return tokens[index].findLast(token => token.offset <= offset)?.type;
}

try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  await test('TOML editor highlights keys, typed values, nested arrays and multiline strings', async t => {
    const page = await fixture(t);
    const tokens = await page.evaluate(() => window.monaco.editor.tokenize(window.editor.getValue(), 'toml'));
    for (const [line, fragment, type] of [
      ['# Project configuration', '#', 'comment'],
      ['[package]', 'package', 'type.identifier'],
      ['name = "aow"', 'name', 'attribute.name'],
      ['name = "aow"', 'aow', 'string'],
      ['enabled = true', 'true', 'keyword'],
      ['count = 1_024', '1_024', 'number'],
      ['ratio = -1.25e+2', '-1.25e+2', 'number'],
      ['hex = 0xCA_FE', '0xCA_FE', 'number'],
      ['octal = 0o755', '0o755', 'number'],
      ['mask = 0b1010_0101', '0b1010_0101', 'number'],
      ['infinite = +inf', '+inf', 'number'],
      ['date = 1979-05-27T07:32:00Z', '1979', 'number.date'],
      ['time = 07:32:00.123', '07:32', 'number.date'],
      ['dependency = { version = "1", features = ["derive"], optional = false }', 'optional', 'attribute.name'],
      ['"quoted.key".enabled = true', 'quoted.key', 'attribute.name'],
      ["3.14159 = 'pi'", '3.14159', 'attribute.name'],
      [String.raw`escaped = "tab\tunicode\u4E2D"`, '\\u4E2D', 'string.escape'],
      ['multiline = """first # still a string', '#', 'string'],
      [String.raw`key = \"still text\"`, 'key', 'string'],
      [String.raw`literal = '''C:\temp # still a string`, '#', 'string'],
      ['name = "not a key"\'\'\'', 'name', 'string'],
      ['after = false # real comment', 'after', 'attribute.name'],
      ['after = false # real comment', '#', 'comment'],
      ['  [1, 2],', '1', 'number'],
      ['  [3, 4],', '3', 'number'],
      ['[[bin]]', 'bin', 'type.identifier'],
      ["[target.'cfg(unix)'.dependencies]", 'cfg(unix)', 'type.identifier'],
    ]) assert.equal(tokenType(tokens, line, fragment), `${type}.toml`, `${line}: ${fragment}`);

    await page.evaluate(async () => {
      window.editor.setSelection(new window.monaco.Range(3, 1, 3, 1));
      await window.editor.getAction('editor.action.commentLine').run();
    });
    assert.equal(await page.evaluate(() => window.editor.getValue().split('\n')[2]), '# name = "aow"');
    const recovery = await page.evaluate(() => window.monaco.editor.tokenize('[unfinished\nname = "ok"\n', 'toml'));
    assert.equal(recovery[1][0].type, 'attribute.name.toml', 'an incomplete table header does not recolor later assignments');
  });
  await test('TOML diff highlights both sides when opened before a normal file editor', async t => {
    const page = await fixture(t, { diff: true });
    assert.deepEqual(await page.evaluate(() => {
      const model = window.diffEditor.getModel();
      return [model.original.getLanguageId(), model.modified.getLanguageId()];
    }), ['toml', 'toml']);
    for (const side of ['original', 'modified']) {
      const colors = await page.locator(`.${side}-in-monaco-diff-editor .view-lines span[class]`).evaluateAll(spans => [...new Set(spans.map(span => getComputedStyle(span).color))]);
      assert.ok(colors.length >= 3, `${side}: ${colors}`);
    }
  });
  await test('JavaScript and TypeScript module extensions highlight in editors and diffs', async t => {
    for (const [filename, language] of [['entry.mjs', 'javascript'], ['entry.cjs', 'javascript'], ['entry.mts', 'typescript'], ['entry.cts', 'typescript']]) {
      for (const diff of [false, true]) {
        const page = await fixture(t, { filename, language, diff });
        const tokenTypes = await page.evaluate(() => {
          const model = window.editor.getModel();
          return window.monaco.editor.tokenize(model.getValue(), model.getLanguageId()).flat().map(token => token.type);
        });
        for (const type of ['comment', 'keyword', 'string', 'number']) {
          assert.ok(tokenTypes.some(token => token.startsWith(`${type}.`)), `${filename}: ${type}`);
        }
        if (diff) {
          assert.equal(await page.evaluate(() => window.diffEditor.getModel().original.getLanguageId()), language);
        }
      }
    }
  });
  await test('JSONC supports comments, trailing commas and formatting without relaxing JSON validation', async t => {
    for (const diff of [false, true]) {
      const page = await fixture(t, { filename: diff ? 'settings.JSONC' : 'settings.jsonc', language: 'json', diff });
      const result = await page.evaluate(async () => {
        const monaco = window.monaco;
        const model = window.editor.getModel();
        const jsonModel = monaco.editor.createModel(model.getValue(), 'json', monaco.Uri.parse('inmemory://aow/test/plain.json'));
        const invalidModel = monaco.editor.createModel('{"missing": }', 'json', monaco.Uri.parse('inmemory://aow/test/invalid.jsonc'));
        const models = [model, jsonModel, invalidModel, ...(window.diffEditor ? [window.diffEditor.getModel().original] : [])];
        const getWorker = await monaco.json.getWorker();
        const worker = await getWorker(...models.map(model => model.uri));
        const diagnostics = await Promise.all(models.map(model => worker.doValidation(model.uri.toString())));
        const tokens = monaco.editor.tokenize(model.getValue(), 'json').flat().map(token => token.type);
        await window.editor.getAction('editor.action.formatDocument').run();
        const formatted = model.getValue();
        jsonModel.dispose();
        invalidModel.dispose();
        return { diagnostics, tokens, formatted };
      });
      assert.deepEqual(result.diagnostics[0], [], 'JSONC allows comments and trailing commas');
      assert.ok(result.diagnostics[1].some(item => /comment/i.test(item.message)), 'JSON still rejects comments');
      assert.ok(result.diagnostics[1].some(item => /trailing comma/i.test(item.message)), 'JSON still rejects trailing commas');
      assert.ok(result.diagnostics[2].length > 0, 'invalid JSONC still reports syntax errors');
      if (diff) assert.deepEqual(result.diagnostics[3], [], 'the original diff model also supports JSONC');
      assert.ok(result.tokens.some(type => type.startsWith('comment.')), 'comments are highlighted');
      assert.ok(result.tokens.some(type => type.startsWith('string.')), 'strings are highlighted');
      assert.ok(result.tokens.some(type => type.startsWith('number')), 'numbers are highlighted');
      assert.ok(result.formatted.includes('// Configuration') && result.formatted.includes('/* Keep this comment */'), 'formatting preserves comments');
      assert.notEqual(result.formatted, jsonc, 'formatting is available');
    }
  });
} finally {
  await browser?.close();
  await server.close();
}
