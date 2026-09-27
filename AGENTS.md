# Development Guidelines

## Rust and Frontend

- Use Cargo for the Rust workspace and follow the Rust edition and formatting conventions defined in `Cargo.toml` and existing code.
- After changing Rust code, run `cargo fmt --all -- --check` and relevant tests. For changes affecting multiple crates, run `cargo test --workspace`.
- For frontend or VT worker changes, follow the existing npm scripts in the relevant directory and run the applicable build, typecheck, or tests.
- For changes spanning multiple components or delivering a complete feature, use the validation targets in `justfile`: `just build` and `just test`.
- If validation cannot be run because of environment or dependency issues, state that clearly; do not claim it passed.

## Pull Request Requirements

- Both the pull request title and description must be written in English. This requirement does not apply to code, commit messages, or other discussion.
- Keep the title concise and descriptive. The description should explain the context, key changes, and validation performed. Clearly disclose any validation that was not run or failed.
- Review the diff before submitting a pull request and ensure it contains only changes relevant to the task.
