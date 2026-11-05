import { stageRipgrep } from '../host/scripts/ripgrep.mjs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
export { stageRipgrep };
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [target, destination] = process.argv.slice(2);
  if (!target || !destination) throw new Error('Usage: node scripts/ripgrep.mjs TARGET DESTINATION');
  stageRipgrep(target, resolve(destination));
}
