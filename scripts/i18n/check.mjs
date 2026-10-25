import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import { placeholders, validateFrontendSource, validateRustSource } from './audit.mjs';

function flatten(value, prefix = '', result = {}) {
  for (const [key, item] of Object.entries(value)) {
    const full = prefix ? `${prefix}.${key}` : key;
    if (typeof item === 'string') result[full] = item;
    else flatten(item, full, result);
  }
  return result;
}
for (const directory of ['frontend/src/lib/i18n/locales', 'crates/i18n/locales']) {
  const english = flatten(JSON.parse(await readFile(`${directory}/en-US.json`, 'utf8')));
  const files = await readdir(directory);
  for (const file of files.filter(name => name.endsWith('.json'))) {
    const translated = flatten(JSON.parse(await readFile(`${directory}/${file}`, 'utf8')));
    assert.deepEqual(Object.keys(translated).sort(), Object.keys(english).sort(), `${directory}/${file}: key mismatch`);
    for (const key of Object.keys(english)) {
      assert.deepEqual(placeholders(translated[key]), placeholders(english[key]), `${file}: ${key}: placeholder mismatch`);
    }
  }
  console.log(`${directory}: ${Object.keys(english).length} keys, ${files.length} languages`);
}
const technicalTerms = new Set(Object.values(JSON.parse(await readFile('scripts/i18n/technical-terms.json', 'utf8'))).flat());
const resources = flatten(JSON.parse(await readFile('frontend/src/lib/i18n/locales/en-US.json', 'utf8')));
async function scan(directory) {
  for (const item of await readdir(directory, { withFileTypes: true })) {
    const file = `${directory}/${item.name}`;
    if (item.isDirectory()) { if (!['assets', 'i18n'].includes(item.name)) await scan(file); continue; }
    if (!/\.tsx?$/u.test(file)) continue;
    validateFrontendSource(await readFile(file, 'utf8'), file, resources, technicalTerms);
  }
}
await scan('frontend/src');
console.log('Frontend keys and hardcoded text checks passed.');

const backend = JSON.parse(await readFile('crates/i18n/locales/en-US.json', 'utf8'));
async function checkRust(directory) {
  for (const item of await readdir(directory, { withFileTypes: true })) {
    const file = `${directory}/${item.name}`;
    if (item.isDirectory()) { await checkRust(file); continue; }
    if (!file.endsWith('.rs')) continue;
    const source = await readFile(file, 'utf8');
    validateRustSource(source, file, backend);
  }
}
await checkRust('crates');
console.log('Backend translation references passed.');
