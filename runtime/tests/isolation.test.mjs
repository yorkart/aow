import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
test('runtime dependencies stay within runtime or the common contracts', () => {
  for (const entry of readdirSync(join(root, 'crates'))) {
    const manifest = join(root, 'crates', entry, 'Cargo.toml');
    const content = readFileSync(manifest, 'utf8');
    for (const match of content.matchAll(/\bpath\s*=\s*"([^"]+)"/g)) {
      const dependency = relative(root, resolve(dirname(manifest), match[1]));
      assert.ok(!dependency.startsWith('../') || dependency.startsWith('../common/'), `${manifest}: ${match[1]} escapes runtime`);
    }
  }
});

test('terminal execution is independent of its HTTP and WebSocket service', () => {
  const core = join(root, 'crates/terminal-core');
  const service = join(root, 'crates/terminal-service');
  const manifest = readFileSync(join(core, 'Cargo.toml'), 'utf8');
  assert.doesNotMatch(manifest, /\b(?:axum|hyper|hyper-util|http|tokio-tungstenite|aow-terminal-service)\s*=/);
  for (const entry of readdirSync(join(core, 'src'), { recursive: true })) {
    if (!entry.endsWith('.rs')) continue;
    assert.doesNotMatch(readFileSync(join(core, 'src', entry), 'utf8'), /\b(?:axum|hyper|hyper_util|tokio_tungstenite)::|\bIntoResponse\b/);
  }
  const serviceManifest = readFileSync(join(service, 'Cargo.toml'), 'utf8');
  assert.match(serviceManifest, /aow-terminal-core\s*=\s*\{/);
  assert.doesNotMatch(serviceManifest, /\b(?:portable-pty|aow-agents(?:-core)?|aow-process|aow-filesystem)\s*=/);
});

test('Agent and execution core APIs stay independent of their services', () => {
  for (const name of ['agents', 'exec']) {
    const core = join(root, 'crates', `${name}-core`);
    const manifest = readFileSync(join(core, 'Cargo.toml'), 'utf8');
    assert.doesNotMatch(manifest, /\b(?:axum|hyper|http|tokio-tungstenite|aow-[\w-]*service|aow-runtime-daemon)\s*=/);
    for (const entry of readdirSync(join(core, 'src'), { recursive: true })) {
      if (!entry.endsWith('.rs')) continue;
      assert.doesNotMatch(readFileSync(join(core, 'src', entry), 'utf8'), /\b(?:axum|hyper|tokio_tungstenite)::|\bIntoResponse\b/);
    }
    const service = readFileSync(join(root, 'crates', `${name}-service`, 'Cargo.toml'), 'utf8');
    assert.match(service, new RegExp(`aow-${name}-core\\s*=`));
    assert.doesNotMatch(service, /\b(?:rusqlite|portable-pty|aow-runtime-process)\s*=/);
  }
  const agents = readFileSync(join(root, 'crates/agents-core/Cargo.toml'), 'utf8');
  assert.doesNotMatch(agents, /\b(?:aow-terminal-core|aow-exec-core)\s*=/);
  const process = readFileSync(join(root, 'crates/process/Cargo.toml'), 'utf8');
  assert.doesNotMatch(process, /\baow-(?:agents|exec|terminal)-/);
  const daemon = readFileSync(join(root, 'crates/runtime-daemon/src/lib.rs'), 'utf8');
  assert.doesNotMatch(daemon, /\bmod (?:agents|sessions|completions|executions);/);
});
