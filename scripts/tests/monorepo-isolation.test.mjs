import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
for (const product of ['common', 'server', 'host']) {
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
  assert.deepEqual(readdirSync(resolve(root, 'common/crates')).sort(), ['host-contract', 'i18n', 'id', 'macos-log', 'sqlite']);
  const contract = readFileSync(resolve(root, 'common/crates/host-contract/Cargo.toml'), 'utf8');
  assert.doesNotMatch(contract, /rusqlite|tokio|reqwest|agent-client-protocol|aow-(?:acp|storage|filesystem|agent-runtime|agents|process|sqlite|macos-log)/);
  for (const name of ['sqlite', 'macos-log']) {
    const manifest = readFileSync(resolve(root, 'common/crates', name, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /\baow-[\w-]+\s*=/);
  }
});

test('Host wire contracts and Server business API data have separate owners', () => {
  assert.equal(existsSync(resolve(root, 'common/crates/protocol')), false);
  const source = directory => readdirSync(directory, { withFileTypes: true }).map(entry => {
    const path = resolve(directory, entry.name);
    return entry.isDirectory() ? source(path) : entry.name.endsWith('.rs') ? readFileSync(path, 'utf8') : '';
  }).join('\n');
  const contract = source(resolve(root, 'common/crates/host-contract/src'));
  assert.doesNotMatch(contract, /pub (?:struct|enum) (?:TerminalTab|TerminalPane\b|TerminalLayout|TerminalHosting|AgentTerminalCreate|AcpCreate|CreationContext|InstanceInfo|PullRequest\w*|MyPullRequests)\b/);
  assert.doesNotMatch(contract, /pub (?:remote_url|commit_url):/);
  const business = source(resolve(root, 'server/crates/api-contract/src'));
  for (const name of ['TerminalTab', 'TerminalLayout', 'TerminalHosting', 'AgentTerminalCreate', 'AcpCreate', 'InstanceInfo', 'PullRequestDetail']) {
    assert.match(business, new RegExp(`pub (?:struct|enum) ${name}\\b`));
  }
  const sdk = readFileSync(resolve(root, 'server/crates/host-client/Cargo.toml'), 'utf8');
  assert.doesNotMatch(sdk, /aow-api-contract|aow-protocol/);
  for (const product of ['common', 'host', 'server']) {
    const lock = readFileSync(resolve(root, product, 'Cargo.lock'), 'utf8');
    assert.doesNotMatch(lock, /name = "aow-protocol"/);
    if (product !== 'server') assert.doesNotMatch(lock, /name = "aow-api-contract"/);
  }
});
test('products share the macOS adapter while operation history stays in Server', () => {
  for (const product of ['server', 'host']) {
    assert.equal(existsSync(resolve(root, product, 'crates/macos-log')), false);
  }
  for (const path of ['server/crates/app', 'host/crates/host-daemon', 'host/crates/terminal-core']) {
    const manifest = readFileSync(resolve(root, path, 'Cargo.toml'), 'utf8');
    assert.match(manifest, /aow-macos-log\s*=\s*\{\s*path\s*=\s*"\.\.\/\.\.\/\.\.\/common\/crates\/macos-log"\s*\}/);
  }
  assert.ok(existsSync(resolve(root, 'server/crates/operation-log')));
});
test('products share SQLite infrastructure while owning business storage', () => {
  for (const product of ['server', 'host']) {
    assert.equal(existsSync(resolve(root, product, 'crates/db/src/sqlite')), false);
    const manifest = readFileSync(resolve(root, product, 'crates/db/Cargo.toml'), 'utf8');
    assert.match(manifest, /aow-sqlite\s*=\s*\{\s*path\s*=\s*"\.\.\/\.\.\/\.\.\/common\/crates\/sqlite"\s*\}/);
  }
  for (const path of ['server/crates/db/src/tasks', 'host/crates/db/src/acp', 'host/crates/db/src/codex_like', 'host/crates/db/src/hermes']) {
    assert.ok(existsSync(resolve(root, path)), path);
  }
});
test('Server consumes ACP through the remote SDK without linking an execution engine', () => {
  assert.equal(existsSync(resolve(root, 'server/crates/acp-client-model')), false);
  assert.equal(existsSync(resolve(root, 'server/crates/db/src/acp')), false);
  for (const entry of readdirSync(resolve(root, 'server/crates'))) {
    const manifest = readFileSync(resolve(root, 'server/crates', entry, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /aow-acp(?:\s|\W)|acp-client-model/);
    if (entry !== 'host-client') assert.doesNotMatch(manifest, /agent-client-protocol/);
  }
  for (const entry of readdirSync(resolve(root, 'server/crates/host-client/src/acp'))) {
    assert.doesNotMatch(readFileSync(resolve(root, 'server/crates/host-client/src/acp', entry), 'utf8'), /\bAcpService\b|\bAcpHost\b|rusqlite|Command::new/);
  }
});
test('the root orchestrates products without flattening their sources', () => {
  assert.equal(existsSync(resolve(root, 'Cargo.toml')), false);
  assert.equal(existsSync(resolve(root, 'crates')), false);
  assert.equal(existsSync(resolve(root, 'frontend')), false);
});

test('Server reaches native agents only through Host contracts and the SDK', () => {
  for (const path of ['server/crates/agents', 'server/crates/db/src/codex_like', 'server/crates/db/src/hermes', 'server/crates/app/src/terminal/sessions/local.rs']) {
    assert.equal(existsSync(resolve(root, path)), false, path);
  }
  for (const entry of readdirSync(resolve(root, 'server/crates'))) {
    const manifest = readFileSync(resolve(root, 'server/crates', entry, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /aow-(?:host-)?(?:agent-runtime|agents|exec)(?:-core|-service)?\s*=/);
  }
  const controller = readFileSync(resolve(root, 'server/crates/app/src/terminal/agent_control/connection.rs'), 'utf8');
  assert.doesNotMatch(controller, /200~|201~|input_ready/);
  for (const path of ['agents.rs', 'sessions.rs']) {
    const sdk = readFileSync(resolve(root, 'server/crates/host-client/src', path), 'utf8');
    assert.doesNotMatch(sdk, /Command::new|rusqlite|AgentInteractive|AgentAutomation|AgentSessionProvider/);
  }
});

test('workspace filesystem execution and bundled search belong only to Host', () => {
  for (const path of ['server/crates/filesystem', 'server/vendor/ripgrep', 'server/scripts/ripgrep.mjs']) {
    assert.equal(existsSync(resolve(root, path)), false, path);
  }
  for (const entry of readdirSync(resolve(root, 'server/crates'))) {
    const manifest = readFileSync(resolve(root, 'server/crates', entry, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /aow-(?:host-)?filesystem\s*=/);
  }
  for (const path of ['node.rs', 'content.rs', 'files.rs', 'entries.rs', 'raw.rs', 'search/mod.rs']) {
    const handler = readFileSync(resolve(root, 'server/crates/app/src/filesystem', path), 'utf8');
    assert.doesNotMatch(handler, /aow_filesystem|(?:std|tokio)::fs|Command::new/);
  }
  assert.ok(existsSync(resolve(root, 'host/crates/filesystem/Cargo.toml')));
  assert.ok(existsSync(resolve(root, 'host/vendor/ripgrep/manifest.json')));
});

test('Server has no workspace, process, role HOME or clipboard execution copies', () => {
  for (const path of ['server/crates/git-service', 'server/crates/workspaces', 'server/crates/process', 'server/crates/app/src/roles/home_fixture.rs', 'server/crates/app/src/roles/identity_fixture.rs', 'server/crates/app/src/terminal/clipboard/storage.rs', 'server/crates/app/src/git_platform_providers/adapter_fixture.rs']) {
    assert.equal(existsSync(resolve(root, path)), false, path);
  }
  const client = readFileSync(resolve(root, 'server/crates/host-client/src/client.rs'), 'utf8');
  assert.doesNotMatch(client, /pub async fn (?:get_json|post_json|put_json|delete|websocket)\b/);
  assert.doesNotMatch(readFileSync(resolve(root, 'server/crates/host-client/src/workspace.rs'), 'utf8'), /pub async fn workspace_operation/);
});

test('Server business services own their state without a product-wide manager', () => {
  const app = resolve(root, 'server/crates/app/src');
  assert.equal(existsSync(resolve(app, 'aow')), false);
  const state = readFileSync(resolve(app, 'state.rs'), 'utf8');
  const routes = readFileSync(resolve(app, 'routes.rs'), 'utf8');
  for (const domain of ['projects', 'agents', 'roles', 'tasks', 'settings']) {
    assert.ok(existsSync(resolve(app, domain, 'service.rs')), domain);
    assert.ok(existsSync(resolve(app, domain, 'api.rs')), domain);
    assert.match(state, new RegExp(`pub\\(crate\\) ${domain}: crate::${domain}::`));
    assert.match(routes, new RegExp(`\\.merge\\(crate::${domain}::routes\\(\\)\\)`));
    const service = readFileSync(resolve(app, domain, 'service.rs'), 'utf8');
    assert.doesNotMatch(service, /pub\(crate\) inner:/);
  }
  const inspect = directory => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = resolve(directory, entry.name);
      if (entry.isDirectory()) inspect(path);
      else if (entry.name.endsWith('.rs')) {
        const source = readFileSync(path, 'utf8');
        assert.doesNotMatch(source, /\bAowManager\b|\bAowInner\b|crate::aow::/, path);
        if (!path.startsWith(resolve(app, 'tasks') + '/') && !entry.name.endsWith('tests.rs')) {
          assert.doesNotMatch(source, /\bTaskDb\b|\.tasks\s*\.inner\b/, path);
        }
      }
    }
  };
  inspect(app);
  assert.ok(existsSync(resolve(app, 'tasks/workflow.rs')));
  for (const operation of ['creation', 'removal']) {
    assert.ok(existsSync(resolve(app, 'projects/worktrees', operation, 'mod.rs')));
  }
});
test('products share only the locale engine and own independent catalogs', () => {
  for (const product of ['server', 'host']) {
    assert.equal(existsSync(resolve(root, product, 'crates/i18n/src/language.rs')), false);
    assert.match(readFileSync(resolve(root, product, 'crates/i18n/Cargo.toml'), 'utf8'), /aow-i18n-core/);
  }
  const server = JSON.parse(readFileSync(resolve(root, 'server/crates/i18n/locales/en-US.json')));
  const runtime = JSON.parse(readFileSync(resolve(root, 'host/crates/i18n/locales/en-US.json')));
  assert.notDeepEqual(Object.keys(server), Object.keys(runtime));
  assert.ok(server['server.untitled_project']);
  assert.equal(runtime['server.untitled_project'], undefined);
});
