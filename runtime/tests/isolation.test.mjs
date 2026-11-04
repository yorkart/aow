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
