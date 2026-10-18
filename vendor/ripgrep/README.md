# Bundled ripgrep

AoW vendors the unmodified upstream release archives for Linux (static musl)
and macOS, on x86_64 and ARM64. `manifest.json` pins the version, source URLs,
archive directory names, and SHA-256 checksums from the GitHub release API.

`scripts/ripgrep.mjs` verifies the archive before extracting it. Cargo stages
the target tool in its build output for development. Release packaging copies
only the target platform's executable to `bin/rg`, together with the upstream
license files in `licenses/ripgrep`. Packaging also checks native linkage and
the minimum macOS version. Downloads are not required during builds or installs.

To update, download all four matching artifacts from the official
[ripgrep releases](https://github.com/BurntSushi/ripgrep/releases), verify their
SHA-256 checksums, replace the archives and manifest, and run the ripgrep and
release packaging tests. Keep upstream license notices with every release.

The server resolves `rg` beside its own executable first, then the development
build output in debug builds, then an absolute executable found on `PATH`.
No search falls back to a different search engine. Missing installations report
how to repair AoW or install ripgrep on the server host.
