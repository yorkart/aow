import assert from 'node:assert/strict';
import { readdir, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import ts from 'typescript';

const root = fileURLToPath(new URL('../', import.meta.url));
const src = path.join(root, 'src');
async function filesUnder(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  return (await Promise.all(entries.map(entry => entry.isDirectory()
    ? filesUnder(path.join(directory, entry.name)) : path.join(directory, entry.name)))).flat();
}
const files = [...await filesUnder(src), ...await filesUnder(path.join(root, 'tests'))];

test('local module imports resolve and source paths are portable across case-insensitive filesystems', async () => {
  const names = new Set();
  for (const file of files.filter(file => file.startsWith(src + path.sep))) {
    const name = path.relative(src, file).toLowerCase();
    assert.ok(!names.has(name), `Conflicting source path: ${path.relative(src, file)}`);
    names.add(name);
  }
  const errors = [];
  for (const file of files.filter(file => /\.(?:tsx?|mjs)$/.test(file))) {
    const source = ts.createSourceFile(file, await readFile(file, 'utf8'), ts.ScriptTarget.Latest, true);
    const imports = [];
    const visit = node => {
      if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) imports.push(node.moduleSpecifier.text);
      if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword && ts.isStringLiteral(node.arguments[0])) imports.push(node.arguments[0].text);
      ts.forEachChild(node, visit);
    };
    visit(source);
    for (const specifier of imports) {
      if (!specifier.startsWith('.') && !specifier.startsWith('/src/')) continue;
      const name = specifier.split('?')[0];
      const target = name.startsWith('/src/') ? path.join(root, name) : path.resolve(path.dirname(file), name);
      const candidates = [target, `${target}.ts`, `${target}.tsx`, `${target}/index.ts`, `${target}/index.tsx`];
      if (!(await Promise.all(candidates.map(candidate => stat(candidate).then(value => value.isFile()).catch(() => false)))).some(Boolean)) {
        errors.push(`${path.relative(root, file)}: missing ${specifier}`);
      }
      const owner = path.relative(src, file).split(path.sep)[0];
      const dependency = path.relative(src, target).split(path.sep)[0];
      if (owner === 'components' && !['components', 'lib'].includes(dependency)
        || owner === 'lib' && dependency !== 'lib') {
        errors.push(`${path.relative(root, file)}: ${owner} must not depend on ${specifier}`);
      }
    }
  }
  assert.deepEqual(errors, []);
});
