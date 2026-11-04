# Server

Server owns business APIs, CLI, automation scheduling and the web frontend. This
is an independent Cargo workspace. No production path dependencies refer to
`../runtime` or any parent source directory.

```sh
npm --prefix frontend ci --ignore-scripts --no-audit --no-fund
npm --prefix frontend run build
cargo build --workspace --locked
cargo run -p aow-server -- --runtime-url http://node:9760 \
  --runtime-token-file /private/runtime-token --server-url https://aow.example.com
```

`--server-url` (or `AOW_SERVER_PUBLIC_URL`) is the externally reachable Server
base URL, including any configured base path. Agent callbacks must be able to
reach it. The default is the local listener on loopback and is intended for
single-machine development.

```sh
AOW_SERVER_URL=https://aow.example.com \
AOW_SERVER_TOKEN_FILE=/private/cli-token aow-cli project list
```

Server creates a private master CLI token in `<state>/runtime/cli/token` unless
`--cli-token-file` / `AOW_SERVER_CLI_TOKEN_FILE` specifies another private file.
CLI tokens authorize `/api/cli/v1/*` only. Browser Cookies and runtime tokens do
not authorize those endpoints; CLI tokens cannot access browser administration
APIs. Use HTTPS outside a trusted private network. Never pass tokens as arguments.

Authenticated browser administrators can issue expiring CLI tokens with
`POST /api/cli-tokens` (`{"ttl_seconds":86400}`) and revoke a grant with
`DELETE /api/cli-tokens/{id}`. The response includes the ID, token and expiry.
Replacing the master token revokes every previously signed grant. Grant metadata
persists in the Server state directory across restarts.

Server injects `AOW_SERVER_URL` and a 24-hour CLI grant into CLI/ACP agent
environments. Shared ACP adapters reuse a valid grant to retain their process identity. Scheduled execution obtains a new grant for each run. runtime
forwards those environment values without implementing or importing Server
business code. Expired/revoked agents need a new launch to receive a fresh grant.
Local same-user Unix socket access and read-only offline automation inspection
remain available when no remote connection is configured. A failed remote
connection never falls back to local state.

For black-box caller tests, build the independent runtime fixture separately and
provide `AOW_RUNTIME_TEST_HOST=/absolute/path/runtime-test-host`. The monorepo root
`just test` handles this preparation. From this directory use `cargo test
--workspace --locked`, `cargo fmt --all -- --check` and `cargo clippy --workspace
--all-targets --locked -- -D warnings`.
