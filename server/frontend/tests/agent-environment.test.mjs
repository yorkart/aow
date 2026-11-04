import { loadModule } from './helpers/load-module.mjs';
import assert from 'node:assert/strict';
import { test } from 'node:test';

const { environmentDraft, parseEnvironment } = await loadModule(new URL('../src/features/agents/environment.ts', import.meta.url));
const parse = text => parseEnvironment({ text, values: {} });

test('environment entries preserve equals signs, empty values, whitespace, and literal shell syntax', () => {
  assert.deepEqual(parse('\r\n BASE_URL =https://example.com/api?a=b=c\r\nEMPTY=\nKEY=  spaced value  \nQUOTES="literal"\nPATH_VALUE=C:\\new\\folder\nEXPAND=$HOME\n \t'), {
    BASE_URL: 'https://example.com/api?a=b=c', EMPTY: '', KEY: '  spaced value  ',
    QUOTES: '"literal"', PATH_VALUE: 'C:\\new\\folder', EXPAND: '$HOME',
  });
  assert.deepEqual(parse(' \n\r\n'), {});
  assert.deepEqual(parse('__proto__=literal\nconstructor=value'), Object.fromEntries([['__proto__', 'literal'], ['constructor', 'value']]));
});

test('existing environments round trip without losing multiline or literal values during unrelated edits', () => {
  const values = { EMPTY: '', URL: 'https://example.com/?a=b', MULTILINE: 'first\\literal\r\nsecond\nthird', LITERAL: String.raw`first\nsecond` };
  const draft = environmentDraft(values);
  assert.equal(draft.text.split('\n').length, 4, 'every existing variable occupies one display line');
  assert.deepEqual(parseEnvironment(draft), values);
  assert.deepEqual(parseEnvironment({ ...draft, text: `${draft.text}\nNEW=value` }), { ...values, NEW: 'value' });
  assert.deepEqual(parseEnvironment({ ...draft, text: draft.text.replace('EMPTY=', 'EMPTY=updated') }), { ...values, EMPTY: 'updated' });
});

test('invalid lines and duplicate keys report line numbers without exposing environment values', () => {
  for (const line of ['MISSING_SEPARATOR', '=secret', 'BAD-KEY=secret', '{"KEY":"secret"}', 'KEY=secret\0value']) {
    assert.throws(() => parse(`VALID=value\n${line}`), error => /第 2 行/.test(error.message) && !error.message.includes('secret'));
  }
  assert.throws(() => parse('KEY=first\nKEY=second'), /第 2 行的变量名重复/);
});
