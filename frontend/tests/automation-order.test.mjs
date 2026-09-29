import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('../src/features/automations/runOrder.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } });
const { newestRunFirst } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

test('mixed legacy and Snowflake runs keep the same chronological order as server cursors', () => {
  const middle = ((BigInt(Date.parse('2026-09-29T12:00:00Z')) - 1288834974657n) << 22n).toString(36);
  const oldest = '20260928T120000000Z_9999';
  const newest = '20260930T120000000Z_0000';
  const rows = [oldest, middle, newest].map(id => ({ id }));
  assert.deepEqual(rows.sort(newestRunFirst).map(row => row.id), [newest, middle, oldest]);
});

test('sorting preserves the full Snowflake sequence and handles encoding length changes', () => {
  const values = [36n ** 12n - 1n, 36n ** 12n, 36n ** 12n + 1n];
  const rows = values.map(id => ({ id: id.toString(36) }));
  assert.deepEqual(rows.sort(newestRunFirst).map(row => row.id), values.reverse().map(id => id.toString(36)));
  assert.deepEqual(['run-1', 'run-3', 'run-2'].map(id => ({ id })).sort(newestRunFirst).map(row => row.id), ['run-3', 'run-2', 'run-1']);
});
