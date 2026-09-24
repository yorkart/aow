import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('../src/features/sessions/sessionProcessPresentation.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } });
const { groupSessionActivities, toolSummary, toolGroupStatus } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const tool = (id, text, extra = {}) => ({ id, kind: 'tool', text, status: 'completed', timestamp: null, ...extra });

test('only adjacent tools merge, preserving order and commentary boundaries without mutating input', () => {
  const activities = [tool('1', 'Read'), tool('2', 'exec_command'), { id: '3', kind: 'commentary', text: 'Next step' }, tool('4', 'Edit')];
  const original = structuredClone(activities);
  const groups = groupSessionActivities(activities);
  assert.deepEqual(groups.map((group) => [group.kind, group.id]), [['tools', '1'], ['commentary', '3'], ['tools', '4']]);
  assert.deepEqual(groups[0].tools.map((item) => item.id), ['1', '2']);
  assert.deepEqual(activities, original);
  assert.deepEqual(groupSessionActivities([]), []);
  const extended = groupSessionActivities([...activities, tool('5', 'Read')]);
  assert.equal(extended[2].id, groups[2].id, 'appending tools preserves the group identity');
});

test('English summaries deduplicate actions and use command semantics instead of generic tool names', () => {
  assert.equal(toolSummary([
    tool('1', 'exec'), tool('2', 'functions.tool_search'),
    tool('3', 'exec_command', { actions: ['read_files', 'read_files'] }),
    tool('4', 'Read'), tool('5', 'apply_patch'), tool('6', 'Edit'),
    tool('7', 'exec_command'), tool('8', 'wait'),
  ]), 'Loaded tools, read files, edited files, ran commands');
  assert.equal(toolSummary([tool('1', 'exec_command', { actions: ['search_files', 'list_files'] })]), 'Searched content, listed files');
});

test('legacy, unknown and wrapper-only calls have honest fallback summaries', () => {
  assert.equal(toolSummary([tool('1', 'functions.exec'), tool('2', 'wait')]), 'Executed tools, waited for results');
  assert.equal(toolSummary([tool('1', 'mcp__server__new_tool')]), 'Called tools');
  assert.equal(toolSummary([tool('1', 'Bash', { actions: ['future_action'] })]), 'Ran commands');
  assert.equal(toolSummary([tool('1', 'Read'), tool('2', 'unknown_tool')]), 'Read files, called tools');
});

test('group status keeps failures visible while other tools run or succeed', () => {
  assert.deepEqual(toolGroupStatus([tool('1', 'Read'), tool('2', 'Edit')]), { status: 'completed', label: '已完成' });
  assert.deepEqual(toolGroupStatus([
    tool('1', 'Read'), tool('2', 'Edit', { status: 'failed' }), tool('3', 'exec_command', { status: 'in_progress' }),
  ]), { status: 'in_progress', label: '1 次执行中 · 1 次失败' });
  assert.deepEqual(toolGroupStatus([tool('1', 'Read'), tool('2', 'Edit', { status: 'failed' })]), { status: 'failed', label: '1 次失败' });
  assert.deepEqual(toolGroupStatus([tool('1', 'Read', { status: undefined }), tool('2', 'Edit', { status: 'interrupted' })]),
    { status: 'interrupted', label: '1 次中断 · 1 次结果未记录' });
});
