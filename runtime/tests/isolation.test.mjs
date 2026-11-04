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
  assert.doesNotMatch(serviceManifest, /\b(?:portable-pty|aow-agents|aow-process|aow-filesystem)\s*=/);
});
