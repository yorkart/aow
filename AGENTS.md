# Development Guidelines

## Git Worktrees

- Create and manage all Git worktrees outside the main repository directory, as sibling directories under the same parent directory (for example, `../aow-feature`). Never place worktrees inside the main repository directory.

## Rust and Frontend

- Use Cargo for the Rust workspace and follow the Rust edition and formatting conventions defined in `Cargo.toml` and existing code.
- Treat `lib.rs` as the crate root: limit it to crate-level documentation and attributes, module declarations, and re-exports. Place implementation and business logic in dedicated modules.
- For modules with child modules, use a consistent directory-based module layout: place the module root and all child module source files within that module's directory. Do not use a mixed layout that places the module root file alongside a same-named module directory.
- After changing Rust code, run `cargo fmt --all -- --check` and relevant tests. For changes affecting multiple crates, run `cargo test --workspace`.
- For frontend or VT worker changes, follow the existing npm scripts in the relevant directory and run the applicable build, typecheck, or tests.
- For changes spanning multiple components or delivering a complete feature, use the validation targets in `justfile`: `just build` and `just test`, subject to the regression-test restriction below.
- Do not run installation and release script regression tests (`node --test scripts/tests/*.test.mjs`) during normal development. Run them only when developing or modifying installation or release script functionality. Since `just test` includes these tests, run its remaining validation steps individually for unrelated changes.
- If validation cannot be run because of environment or dependency issues, state that clearly; do not claim it passed.

## Lint and Static Analysis

- Before committing, run the checks below for every language changed. Changes to shared lint configuration require the full affected language check. Include source code, tests, build scripts, and extensionless launchers; exclude generated output and third-party dependencies.
- Treat lint warnings as failures. Inspect compiler output as well as the exit status. Fix findings before submitting a PR; use a narrowly scoped suppression with a concrete reason only when the code intentionally requires the flagged behavior. Do not disable a rule globally to bypass a new finding.
- Run commands from the repository root. Use Node.js 22.16+ (22.x) or a newer supported LTS, install Node lint dependencies with `npm ci --ignore-scripts --no-audit --no-fund`, and install `scripts/lint/requirements.txt` into a Python virtual environment on `PATH`. Use actionlint 1.7.12 for workflow checks. Tool versions and lint rules are maintained in the repository configuration files.
- Rust checks require the VT bundle first: `npm --prefix vt-worker ci --ignore-scripts --no-audit --no-fund` and `npm --prefix vt-worker run build`. Install the `rustfmt` and `clippy` components for the active stable Rust toolchain.

| Language or format | Required checks |
| --- | --- |
| Rust | `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`. CI checks both Linux and macOS conditional code. |
| TypeScript / TSX | `npm run lint:js` and `npm --prefix frontend run typecheck`. Install frontend dependencies before type checking. |
| JavaScript / MJS | `npm run lint:js`; ESLint covers the frontend, VT worker, scripts, and tests, including React Hook call-order checks. |
| CSS | `npm run lint:css`. Preserve intentional cascade ordering and the existing compact style. |
| HTML | `npm run lint:html`, including browser preview fixtures. |
| Python | `npm run lint:python` (Ruff), including installers, adapters, and test helpers. |
| Shell | `npm run lint:shell`; this checks syntax with the declared shell and runs ShellCheck on scripts and `packaging/bin` launchers. |
| C | On macOS, run `clang -Wall -Wextra -Werror -fsyntax-only crates/macos-log/src/os_log.c` and `clang --analyze -Xanalyzer -analyzer-output=text -Xanalyzer -analyzer-werror -Wall -Wextra -Werror crates/macos-log/src/os_log.c`. Other hosts must disclose that the macOS SDK check was not run. |
| JSON / TOML / YAML | `npm run lint:config` validates repository configuration, including duplicate YAML keys; also run `npm run lint:yaml` for YAML style and structure. |
| GitHub Actions | `npm run lint:workflows` (actionlint with ShellCheck available on `PATH`). |
| Markdown | `npm run lint:markdown` for the configured markup checks. |

- `npm run lint` runs the Node-based language and document checks together. Python, shell, workflow, Rust, and C checks remain explicit commands as listed above.
- CI must invoke Cargo, npm, Python, ShellCheck, Clang, and actionlint directly or through the checked-in scripts; CI must not install or depend on `just`. All applicable lint jobs must pass before publishing a release.
- Lint and type checks complement the build and test requirements above; a passing formatter alone does not satisfy lint requirements.

## Pull Request Requirements

- Both the pull request title and description must be written in English. This requirement does not apply to code, commit messages, or other discussion.
- Keep the title concise and descriptive. The description should explain the context, key changes, and validation performed. Clearly disclose any validation that was not run or failed.
- Review the diff before submitting a pull request and ensure it contains only changes relevant to the task.
