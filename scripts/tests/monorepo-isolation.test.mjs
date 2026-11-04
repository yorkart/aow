import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
for (const product of ['server', 'runtime']) {
  test(`${product} path dependencies stay within its independent source tree`, () => {
    const directory = resolve(root, product);
    for (const entry of readdirSync(resolve(directory, 'crates'))) {
      const manifest = resolve(directory, 'crates', entry, 'Cargo.toml');
      for (const match of readFileSync(manifest, 'utf8').matchAll(/\bpath\s*=\s*"([^"]+)"/g)) {
        const target = relative(directory, resolve(dirname(manifest), match[1]));
        assert.ok(target !== '..' && !target.startsWith('../'), `${manifest}: ${match[1]} escapes ${product}`);
      }
    }
  });
}
test('the root orchestrates products without flattening their sources', () => {
  assert.equal(existsSync(resolve(root, 'Cargo.toml')), false);
  assert.equal(existsSync(resolve(root, 'crates')), false);
  assert.equal(existsSync(resolve(root, 'frontend')), false);
});
