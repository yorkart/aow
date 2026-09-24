import { lstatSync, realpathSync, renameSync, statSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

// rename replaces a symlink itself, including links to directories, on both
// Linux and macOS. Never unlink the working entry before publishing its replacement.
export function replaceSymlink(source, destination) {
  if (!lstatSync(source).isSymbolicLink()) {
    throw new Error(`Replacement must be a symlink: ${source}`);
  }
  statSync(source); // Refuse to activate a dangling link.
  try {
    if (!lstatSync(destination).isSymbolicLink()) {
      throw new Error(`Refusing to replace an unmanaged entry: ${destination}`);
    }
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  renameSync(source, destination);
}

function isMain() {
  if (!process.argv[1]) return false;
  try {
    // Node resolves the module's real path, while argv can retain a symlinked HOME.
    return import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href;
  } catch {
    return false;
  }
}

if (isMain()) {
  try {
    if (process.argv.length !== 4) throw new Error('Usage: replace-symlink.mjs SOURCE DESTINATION');
    replaceSymlink(process.argv[2], process.argv[3]);
  } catch (error) {
    console.error(`error: ${error.message}`);
    process.exitCode = 1;
  }
}
