import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
for (const product of ['common', 'server', 'runtime']) {
  test(`${product} depends only on its own crates and public contracts`, () => {
    for (const entry of readdirSync(resolve(root, product, 'crates'))) {
      const manifest = resolve(root, product, 'crates', entry, 'Cargo.toml');
      for (const match of readFileSync(manifest, 'utf8').matchAll(/\bpath\s*=\s*"([^"]+)"/g)) {
        const target = relative(root, resolve(dirname(manifest), match[1]));
        assert.ok(target.startsWith(`${product}/`) || target.startsWith('common/'), `${manifest}: forbidden dependency ${match[1]}`);
      }
    }
  });
}
test('Common contains data contracts and IDs, with no runtime or storage implementation', () => {
  assert.deepEqual(readdirSync(resolve(root, 'common/crates')).sort(), ['id', 'protocol', 'runtime-contract']);
  const contract = readFileSync(resolve(root, 'common/crates/runtime-contract/Cargo.toml'), 'utf8');
  assert.doesNotMatch(contract, /rusqlite|tokio|reqwest|agent-client-protocol|aow-(?:acp|storage|filesystem|agents|process)/);
});
test('Server consumes ACP through the remote SDK without linking an execution engine', () => {
  assert.equal(existsSync(resolve(root, 'server/crates/acp-client-model')), false);
  assert.equal(existsSync(resolve(root, 'server/crates/db/src/acp')), false);
  for (const entry of readdirSync(resolve(root, 'server/crates'))) {
    const manifest = readFileSync(resolve(root, 'server/crates', entry, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /aow-acp(?:\s|\W)|acp-client-model/);
    if (entry !== 'runtime-client') assert.doesNotMatch(manifest, /agent-client-protocol/);
  }
  for (const entry of readdirSync(resolve(root, 'server/crates/runtime-client/src/acp'))) {
    assert.doesNotMatch(readFileSync(resolve(root, 'server/crates/runtime-client/src/acp', entry), 'utf8'), /\bAcpService\b|\bAcpHost\b|rusqlite|Command::new/);
  }
});
test('the root orchestrates products without flattening their sources', () => {
  assert.equal(existsSync(resolve(root, 'Cargo.toml')), false);
  assert.equal(existsSync(resolve(root, 'crates')), false);
  assert.equal(existsSync(resolve(root, 'frontend')), false);
});
