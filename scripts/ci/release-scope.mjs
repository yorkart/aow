import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Ordinary product changes run the platform tests and frontend build. Changes
// to packaging, native build configuration or dependencies also build every
// release platform before merge. Main and manual runs always build packages.
export function requiresNativePackages(event, paths = []) {
  if (event !== 'pull_request') return true;
  return paths.some(path =>
    /^(scripts|packaging|\.cargo|vt-worker)\//.test(path)
    || path === '.github/workflows/release.yml'
    || /^(Cargo\.(toml|lock)|rust-toolchain(\.toml)?)$/.test(path)
    || /^crates\/[^/]+\/(Cargo\.toml|build\.rs)$/.test(path)
    || /\.(c|h|m|mm)$/.test(path)
    || /^frontend\/package(-lock)?\.json$/.test(path),
  );
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const event = process.env.GITHUB_EVENT_NAME;
  // checkout fetches the PR merge commit and both parents. Comparing with its
  // first parent includes the whole PR, even after the base branch advances.
  const paths = event === 'pull_request'
    ? execFileSync('git', ['diff', '--name-only', '--no-renames', '-z', 'HEAD^', 'HEAD'], { encoding: 'utf8' })
      .split('\0').filter(Boolean)
    : [];
  const required = requiresNativePackages(event, paths);
  appendFileSync(process.env.GITHUB_OUTPUT, `native-packages=${required}\n`);
  console.log(required ? 'Validate all native release packages.' : 'Run platform tests and frontend checks.');
}
