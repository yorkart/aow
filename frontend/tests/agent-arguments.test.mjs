import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('../src/features/agents/arguments.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } });
const { argumentsDraft, normalizeArguments, pasteArguments } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const parse = (text, executable = 'codex') => normalizeArguments({ text, values: [] }, executable);

test('pasted Codex command preserves TOML quotes through normalization and later edits', () => {
  const text = ['-m gpt-6-luna \\', `  -c 'approval_policy="on-request"' \\`, `  -c 'approvals_reviewer="auto_review"'`].join('\n');
  const expected = ['-m', 'gpt-6-luna', '-c', 'approval_policy="on-request"', '-c', 'approvals_reviewer="auto_review"'];
  const draft = parse(text);
  assert.deepEqual(draft.values, expected);
  assert.equal(draft.text, expected.join('\n'));
  assert.deepEqual(normalizeArguments(draft).values, expected);
  assert.deepEqual(normalizeArguments({ ...draft, text: draft.text.replace('on-request', 'never') }).values,
    expected.map(value => value.replace('on-request', 'never')));
});

test('command syntax handles whitespace, continuation, grouping, escapes and a matching executable', () => {
  for (const [text, expected] of [
    ['-m\tgpt-6-luna   --sandbox workspace-write', ['-m', 'gpt-6-luna', '--sandbox', 'workspace-write']],
    ['codex -m gpt-6-luna', ['-m', 'gpt-6-luna']],
    ['/opt/codex -m gpt-6-luna', ['-m', 'gpt-6-luna']],
    ['"/opt/codex" -m gpt-6-luna', ['-m', 'gpt-6-luna']],
    ['--model=gpt-6-luna --sandbox workspace-write', ['--model=gpt-6-luna', '--sandbox', 'workspace-write']],
    ['--name "model with spaces" --path my\\ folder', ['--name', 'model with spaces', '--path', 'my folder']],
    [String.raw`--name 'it'\''s fine' --config "key=\"a b\""`, ['--name', "it's fine", '--config', 'key="a b"']],
    ['-m gpt-6-luna \\  \r\n  --sandbox workspace-write \\', ['-m', 'gpt-6-luna', '--sandbox', 'workspace-write']],
    ['--name "hello\\\nworld"', ['--name', 'helloworld']],
    ['--name "hello \nworld"', ['--name', 'hello \nworld']],
    [String.raw`"C:\temp"`, [String.raw`C:\temp`]],
    [String.raw`$'first\nsecond'`, ['first\nsecond']],
    [String.raw`--path "C:\Users\name" --literal '$HOME' --empty ''`, ['--path', String.raw`C:\Users\name`, '--literal', '$HOME', '--empty', '']],
  ]) assert.deepEqual(parse(text, '/opt/codex').values, expected, text);
});

test('per-line editing preserves spaces and literal quoting without interpreting stored arguments again', () => {
  const values = ['--model', 'model with spaces', '-c', 'key="a b"', '"literal quotes"', '--literal with spaces', '', "''", 'first\nsecond', ' trailing ', String.raw`C:\Users\name`];
  const draft = argumentsDraft(values);
  assert.deepEqual(normalizeArguments(draft).values, values);
  assert.deepEqual(normalizeArguments({ ...draft, text: `${draft.text}\n--last` }).values, [...values, '--last']);
  assert.deepEqual(normalizeArguments({ ...draft, text: draft.text.replace('--literal with spaces', '--literal with more spaces') }).values,
    values.map(value => value.replace('--literal with spaces', '--literal with more spaces')));
  assert.deepEqual(parse('--model\nmodel with spaces\n\n-c\nkey="a b"\n').values, ['--model', 'model with spaces', '-c', 'key="a b"']);
  assert.deepEqual(parse(' \n\t\n').values, []);
});

test('pasting replaces the selection and leaves partial argument edits literal', () => {
  const draft = argumentsDraft(['--model', 'old value', '-c', 'key="a b"']);
  const replaced = pasteArguments(draft, '-m gpt-6-luna', 0, '--model\nold value'.length, 'codex');
  assert.equal(replaced.draft.text, '-m\ngpt-6-luna\n-c\nkey="a b"');
  assert.equal(replaced.caret, '-m\ngpt-6-luna'.length);
  assert.deepEqual(normalizeArguments(replaced.draft).values, ['-m', 'gpt-6-luna', '-c', 'key="a b"']);
  const start = draft.text.indexOf('old');
  const partial = pasteArguments(draft, 'new model', start, start + 3);
  assert.deepEqual(normalizeArguments(partial.draft).values, ['--model', 'new model value', '-c', 'key="a b"']);
});

test('incomplete command quotes fail without silently changing arguments', () => {
  for (const text of ['-m "incomplete', "-c 'key=\"value\"", '"incomplete']) {
    assert.throws(() => parse(text), /引号未闭合/);
  }
});
