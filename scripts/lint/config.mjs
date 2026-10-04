import { readFileSync } from 'node:fs';
import { extname } from 'node:path';
import { parse as parseToml } from 'smol-toml';
import { parseAllDocuments } from 'yaml';
import { repositoryFiles } from './files.mjs';

let checked = 0;
for (const file of repositoryFiles()) {
  const extension = extname(file);
  if (!['.json', '.toml', '.yaml', '.yml'].includes(extension)) continue;
  try {
    const source = readFileSync(file, 'utf8');
    if (extension === '.json') JSON.parse(source);
    else if (extension === '.toml') parseToml(source);
    else {
      for (const document of parseAllDocuments(source, { uniqueKeys: true })) {
        if (document.errors.length || document.warnings.length) {
          throw new Error([...document.errors, ...document.warnings].map(error => error.message).join('\n'));
        }
      }
    }
    checked++;
  } catch (error) {
    console.error(`${file}: ${error.message}`);
    process.exitCode = 1;
  }
}
console.log(`Checked ${checked} JSON, TOML and YAML files.`);
