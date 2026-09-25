import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import ts from 'typescript';

const source = await readFile(new URL('../src/features/git/commitGraph.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } });
const { buildCommitGraph } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const commit = (id, ...parents) => ({ id, parents });

function verifyHistory(commits) {
  const graph = buildCommitGraph(commits);
  const rows = [...graph.values()];
  const byId = new Map(commits.map(value => [value.id, value]));
  for (let current = commits[0]; current; current = byId.get(current.parents[0])) {
    assert.equal(graph.get(current.id).lane, 0, `${current.id} must stay on HEAD's mainline`);
    assert.equal(graph.get(current.id).color, rows[0].color, `${current.id} must keep HEAD's color`);
  }
  rows.forEach((row, index) => {
    const outputs = row.edges.filter(edge => edge.startsAtNode);
    assert.equal(outputs.length, commits[index].parents.length);
    outputs.forEach((edge, parentIndex) => {
      let lane = edge.to;
      let destination;
      // Follow the rendered line through intervening rows to its actual node.
      for (let next = index + 1; next < rows.length; next++) {
        if (rows[next].incoming.some(line => line.lane === lane)) {
          destination = commits[next].id;
          break;
        }
        const continuation = rows[next].edges.find(line => !line.startsAtNode && line.from === lane);
        assert.ok(continuation, `${commits[index].id} has a broken parent edge`);
        lane = continuation.to;
      }
      const parent = commits[index].parents[parentIndex];
      assert.equal(destination, byId.has(parent) ? parent : undefined);
    });
    if (index + 1 < rows.length) {
      const next = rows[index + 1];
      const inputs = [...next.incoming, ...next.edges.filter(edge => !edge.startsAtNode).map(edge => ({ lane: edge.from, color: edge.color }))];
      assert.deepEqual(row.continuing, inputs.sort((a, b) => a.lane - b.lane), 'adjacent rows must share positions and colors');
    }
    const lanes = [row.lane, ...row.incoming.map(line => line.lane), ...row.edges.flatMap(edge => [edge.from, edge.to])];
    assert.ok(lanes.every(lane => lane >= 0 && lane < row.columns), 'reserve space for every line, including side branches');
  });
  return graph;
}

test('linear history stays in one lane and ends at the root', () => {
  assert.equal(buildCommitGraph([]).size, 0);
  const graph = verifyHistory([commit('head', 'middle'), commit('middle', 'root'), commit('root')]);
  assert.ok([...graph.values()].every(row => row.columns === 1));
  assert.deepEqual(graph.get('root').continuing, []);
});

test('merge history keeps the selected branch first regardless of parent traversal order', () => {
  for (const parents of [
    [commit('feature', 'base'), commit('main', 'base')],
    [commit('main', 'base'), commit('feature', 'base')],
  ]) {
    const graph = verifyHistory([commit('head', 'main', 'feature'), ...parents, commit('base')]);
    assert.equal(graph.get('feature').lane, 1);
    assert.notEqual(graph.get('feature').color, graph.get('head').color);
    assert.equal(graph.get('base').incoming.length, 2, 'branches join at their shared parent');
    assert.equal(graph.get('base').color, graph.get('head').color);
  }
});

test('nested merges and octopus merges preserve every parent and the first-parent mainline', () => {
  const graph = verifyHistory([
    commit('head', 'main', 'feature', 'other'),
    commit('other', 'base'),
    commit('feature', 'feature-before', 'main'),
    commit('feature-before', 'base'),
    commit('main', 'base', 'older-feature'),
    commit('older-feature', 'base'),
    commit('base'),
  ]);
  assert.equal(graph.get('head').continuing.length, 3);
  assert.equal(graph.get('main').lane, 0);
  assert.equal(graph.get('base').incoming.length, 4);
  assert.deepEqual(graph.get('base').continuing, []);
});

test('merging unrelated roots leaves the surviving side branch connected', () => {
  const graph = verifyHistory([commit('head', 'main-root', 'feature'), commit('main-root'), commit('feature', 'other-root'), commit('other-root')]);
  assert.equal(graph.get('feature').lane, 1);
  assert.deepEqual(graph.get('other-root').continuing, []);
});

test('a truncated log continues to offscreen parents without inventing commit nodes', () => {
  const graph = verifyHistory([commit('head', 'main', 'feature'), commit('feature', 'offscreen-base'), commit('main', 'offscreen-base')]);
  assert.equal(graph.size, 3);
  assert.equal(graph.get('main').continuing.length, 2);
});
