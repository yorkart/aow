import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
for (const product of ['common', 'server', 'runtime']) {
  test(`${product} depends only on its own crates and Common libraries`, () => {
    for (const entry of readdirSync(resolve(root, product, 'crates'))) {
      const manifest = resolve(root, product, 'crates', entry, 'Cargo.toml');
      for (const match of readFileSync(manifest, 'utf8').matchAll(/\bpath\s*=\s*"([^"]+)"/g)) {
        const target = relative(root, resolve(dirname(manifest), match[1]));
        assert.ok(target.startsWith(`${product}/`) || target.startsWith('common/'), `${manifest}: forbidden dependency ${match[1]}`);
      }
    }
  });
}
test('Common contains contracts, IDs and generic infrastructure without product implementations', () => {
  assert.deepEqual(readdirSync(resolve(root, 'common/crates')).sort(), ['i18n', 'id', 'macos-log', 'protocol', 'runtime-contract', 'sqlite']);
  const contract = readFileSync(resolve(root, 'common/crates/runtime-contract/Cargo.toml'), 'utf8');
  assert.doesNotMatch(contract, /rusqlite|tokio|reqwest|agent-client-protocol|aow-(?:acp|storage|filesystem|agents|process|sqlite|macos-log)/);
  for (const name of ['sqlite', 'macos-log']) {
    const manifest = readFileSync(resolve(root, 'common/crates', name, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /\baow-[\w-]+\s*=/);
  }
});
test('products share the macOS adapter while operation history stays in Server', () => {
  for (const product of ['server', 'runtime']) {
    assert.equal(existsSync(resolve(root, product, 'crates/macos-log')), false);
  }
  for (const path of ['server/crates/app', 'runtime/crates/runtime-daemon', 'runtime/crates/terminal-core']) {
    const manifest = readFileSync(resolve(root, path, 'Cargo.toml'), 'utf8');
    assert.match(manifest, /aow-macos-log\s*=\s*\{\s*path\s*=\s*"\.\.\/\.\.\/\.\.\/common\/crates\/macos-log"\s*\}/);
  }
  assert.ok(existsSync(resolve(root, 'server/crates/operation-log')));
});
test('products share SQLite infrastructure while owning business storage', () => {
  for (const product of ['server', 'runtime']) {
    assert.equal(existsSync(resolve(root, product, 'crates/db/src/sqlite')), false);
    const manifest = readFileSync(resolve(root, product, 'crates/db/Cargo.toml'), 'utf8');
    assert.match(manifest, /aow-sqlite\s*=\s*\{\s*path\s*=\s*"\.\.\/\.\.\/\.\.\/common\/crates\/sqlite"\s*\}/);
  }
  for (const path of ['server/crates/db/src/tasks', 'runtime/crates/db/src/acp', 'runtime/crates/db/src/codex_like', 'runtime/crates/db/src/hermes']) {
    assert.ok(existsSync(resolve(root, path)), path);
  }
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

test('Server reaches native agents only through Runtime contracts and the SDK', () => {
  for (const path of ['server/crates/agents', 'server/crates/db/src/codex_like', 'server/crates/db/src/hermes', 'server/crates/app/src/terminal/sessions/local.rs']) {
    assert.equal(existsSync(resolve(root, path)), false, path);
  }
  for (const entry of readdirSync(resolve(root, 'server/crates'))) {
    const manifest = readFileSync(resolve(root, 'server/crates', entry, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /aow-(?:runtime-)?(?:agents|exec)(?:-core|-service)?\s*=/);
  }
  const controller = readFileSync(resolve(root, 'server/crates/app/src/terminal/agent_control/connection.rs'), 'utf8');
  assert.doesNotMatch(controller, /200~|201~|input_ready/);
  for (const path of ['agents.rs', 'sessions.rs']) {
    const sdk = readFileSync(resolve(root, 'server/crates/runtime-client/src', path), 'utf8');
    assert.doesNotMatch(sdk, /Command::new|rusqlite|AgentInteractive|AgentAutomation|AgentSessionProvider/);
  }
});

test('workspace filesystem execution and bundled search belong only to Runtime', () => {
  for (const path of ['server/crates/filesystem', 'server/vendor/ripgrep', 'server/scripts/ripgrep.mjs']) {
    assert.equal(existsSync(resolve(root, path)), false, path);
  }
  for (const entry of readdirSync(resolve(root, 'server/crates'))) {
    const manifest = readFileSync(resolve(root, 'server/crates', entry, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /aow-(?:runtime-)?filesystem\s*=/);
  }
  for (const path of ['node.rs', 'content.rs', 'files.rs', 'entries.rs', 'raw.rs', 'search/mod.rs']) {
    const handler = readFileSync(resolve(root, 'server/crates/app/src/filesystem', path), 'utf8');
    assert.doesNotMatch(handler, /aow_filesystem|(?:std|tokio)::fs|Command::new/);
  }
  assert.ok(existsSync(resolve(root, 'runtime/crates/filesystem/Cargo.toml')));
  assert.ok(existsSync(resolve(root, 'runtime/vendor/ripgrep/manifest.json')));
});

test('Server has no workspace, process, role HOME or clipboard execution copies', () => {
  for (const path of ['server/crates/git-service', 'server/crates/workspaces', 'server/crates/process', 'server/crates/app/src/aow/roles/home_fixture.rs', 'server/crates/app/src/aow/roles/identity_fixture.rs', 'server/crates/app/src/terminal/clipboard/storage.rs', 'server/crates/app/src/git_platform_providers/adapter_fixture.rs']) {
    assert.equal(existsSync(resolve(root, path)), false, path);
  }
  const client = readFileSync(resolve(root, 'server/crates/runtime-client/src/client.rs'), 'utf8');
  assert.doesNotMatch(client, /pub async fn (?:get_json|post_json|put_json|delete|websocket)\b/);
  assert.doesNotMatch(readFileSync(resolve(root, 'server/crates/runtime-client/src/workspace.rs'), 'utf8'), /pub async fn workspace_operation/);
});
test('products share only the locale engine and own independent catalogs', () => {
  for (const product of ['server', 'runtime']) {
    assert.equal(existsSync(resolve(root, product, 'crates/i18n/src/language.rs')), false);
    assert.match(readFileSync(resolve(root, product, 'crates/i18n/Cargo.toml'), 'utf8'), /aow-i18n-core/);
  }
  const server = JSON.parse(readFileSync(resolve(root, 'server/crates/i18n/locales/en-US.json')));
  const runtime = JSON.parse(readFileSync(resolve(root, 'runtime/crates/i18n/locales/en-US.json')));
  assert.notDeepEqual(Object.keys(server), Object.keys(runtime));
  assert.ok(server['server.untitled_project']);
  assert.equal(runtime['server.untitled_project'], undefined);
});
