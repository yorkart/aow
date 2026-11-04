import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
function within(directory, path) {
  const name = relative(directory, path);
  return name !== '..' && !name.startsWith('../');
}
for (const product of ['common', 'server', 'runtime']) {
  test(`${product} dependencies stay in its own workspace or common`, () => {
    const directory = resolve(root, product);
    for (const entry of readdirSync(resolve(directory, 'crates'))) {
      const manifest = resolve(directory, 'crates', entry, 'Cargo.toml');
      for (const match of readFileSync(manifest, 'utf8').matchAll(/\bpath\s*=\s*"([^"]+)"/g)) {
        const target = resolve(dirname(manifest), match[1]);
        assert.ok(within(directory, target) || within(resolve(root, 'common'), target), `${manifest}: forbidden dependency ${match[1]}`);
      }
    }
  });
}
test('the root orchestrates three independent Cargo workspaces', () => {
  for (const product of ['common', 'server', 'runtime']) {
    assert.ok(existsSync(resolve(root, product, 'Cargo.toml')));
    assert.ok(existsSync(resolve(root, product, 'Cargo.lock')));
  }
  for (const old of ['Cargo.toml', 'crates', 'frontend', 'server/crates/acp-client-model']) {
    assert.equal(existsSync(resolve(root, old)), false);
  }
});
test('only the Server SDK depends on the shared ACP engine', () => {
  for (const entry of readdirSync(resolve(root, 'server/crates'))) {
    if (entry === 'runtime-client') continue;
    const manifest = readFileSync(resolve(root, 'server/crates', entry, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /aow-acp|agent-client-protocol|runtime-contract/, entry);
  }
});
test('shared ACP client cannot install or start a local adapter', () => {
  const acp = resolve(root, 'common/crates/acp-client');
  for (const path of ['src/node_runtime', 'src/project', 'src/agent_servers/installation.rs', 'src/agent_servers/acp/transport.rs']) {
    assert.equal(existsSync(resolve(acp, path)), false);
  }
  const sources = readdirSync(resolve(acp, 'src'), { recursive: true }).filter(path => path.endsWith('.rs'));
  for (const path of sources) {
    assert.doesNotMatch(readFileSync(resolve(acp, 'src', path), 'utf8'), /(?:tokio|std)::process::Command|spawn_stdio|spawn_transport/);
  }
});

test('runtime SDK management data comes from common contracts', () => {
  for (const module of ['management', 'completions', 'environments', 'executions', 'workspace']) {
    const source = readFileSync(resolve(root, 'server/crates/runtime-client/src', `${module}.rs`), 'utf8');
    assert.doesNotMatch(source, /^pub (?:struct|enum) /m, module);
    assert.match(source, /pub use aow_runtime_contract::/, module);
  }
  const manifest = readFileSync(resolve(root, 'common/crates/runtime-contract/Cargo.toml'), 'utf8');
  assert.doesNotMatch(manifest, /acp-client|runtime-client/, 'wire contracts must not depend on client implementations');
});
