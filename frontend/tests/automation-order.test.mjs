import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('../src/features/automations/runOrder.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } });
const { newestRunFirst, runCursor } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

test('history uses recorded start times regardless of opaque ID spelling', () => {
  const rows = [
    { id: '20990101T120000000Z_old', started_at: '2026-09-26T12:00:00Z' },
    { id: '0', started_at: '2026-09-28T12:00:00Z' },
    { id: '550e8400-e29b-41d4-a716-446655440000', started_at: '2026-09-27T12:00:00Z' },
  ];
  assert.deepEqual(rows.sort(newestRunFirst).map(row => row.id), ['0', '550e8400-e29b-41d4-a716-446655440000', '20990101T120000000Z_old']);
});

test('history keeps server precision and only uses IDs to break equal-time ties', () => {
  const rows = [
    { id: 'z-old', started_at: '2026-09-29T12:00:00.123456700Z' },
    { id: 'A-new', started_at: '2026-09-29T12:00:00.123456701Z' },
    { id: 'Z-tied', started_at: '2026-09-29T14:00:00.123456701+02:00' },
  ];
  assert.deepEqual(rows.sort(newestRunFirst).map(row => row.id), ['Z-tied', 'A-new', 'z-old']);
  assert.equal(newestRunFirst(
    { id: 'same', started_at: '2026-09-29T12:00:00.123Z' },
    { id: 'same', started_at: '2026-09-29T12:00:00.123000000Z' },
  ), 0);
});

test('pagination cursor carries metadata separately and preserves the complete ID', () => {
  const run = { id: 'Future_v2-A', started_at: '2026-09-29T12:00:00.123456701Z' };
  assert.equal(runCursor(run), '2026-09-29T12:00:00.123456701Z/Future_v2-A');
  assert.equal(run.id, 'Future_v2-A');
});
