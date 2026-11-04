# Remote business CLI and deployment acceptance

The CLI connects to Server, which sends execution requests through runtime-client.
The node never imports Server business code or reads Server's database. Configure
one runtime endpoint on Server. Use `--server-url` / `AOW_SERVER_PUBLIC_URL` for the
address reachable by node processes, including the Server base path.

```sh
aow-server --host 0.0.0.0 --port 8282 --state-dir /server/state \
  --runtime-url http://runtime:9760 --runtime-token-file /private/runtime-token \
  --server-url https://aow.example.com/workbench

AOW_SERVER_URL=https://aow.example.com/workbench \
AOW_SERVER_TOKEN_FILE=/private/cli-token aow-cli project list
```

Both `--server` and the environment select HTTP(S). Credentials come from the
private token file or `AOW_SERVER_TOKEN`. A configured remote request never
falls back to another machine's local files or socket. Input files such as
`--task-file` are read on the CLI machine; workspace paths belong to runtime.
Automation queries use Server history when a remote address is configured.

The Server SDK lives in `server/crates/server-client`. It handles business API
transport; `server/crates/runtime-client` handles node transport. CLI credentials
are independent of runtime credentials and browser Cookies. The Server-only
`cli-access` crate signs and persists expiring grants and supports individual
revocation. `/api/cli/v1/*` accepts those grants; browser administration APIs do
not. A browser administrator can issue/revoke grants through `/api/cli-tokens`.
See [Server setup](../../server/README.md) for token bootstrap and rotation.

CLI agent launches receive grants from Server. Shared ACP adapter processes reuse
a valid cached grant so merely querying a connection does not restart its process.
Failure to obtain host credentials fails ACP creation rather than silently
starting a process with unusable callbacks. Automation runners issue fresh grants
from Server state for each execution, overriding old launch snapshots. The node
receives only the request environment, without the signing key or grant database.

## Linux isolation acceptance

After building binaries and the VT worker, run from the monorepo root:

```sh
cargo build --manifest-path server/Cargo.toml --workspace --target-dir target
cargo build --manifest-path runtime/Cargo.toml --workspace --target-dir target
npm --prefix runtime/vt-worker run build
node scripts/acceptance/remote-deployment.mjs
```

The explicit acceptance script uses the local Docker daemon and creates an
isolated internal network plus separate Server and runtime containers. It builds
a temporary Ubuntu test image with Git and the current binaries/worker. Containers
share no state mounts and cannot see each other's workspace paths. The test:

1. Registers a repository that exists only on runtime through Server's HTTP API.
2. Saves a manual automation through the business API.
3. Runs its detached runner inside the Server container.
4. Executes a fixture agent on runtime, where `aow-cli` calls Server over HTTP.
5. Checks the callback result and Server's durable completion journal.
6. Starts a native ACP fixture through the remote CLI and checks its independent
   CLI callback, exercising the ACP connector's environment forwarding.

Containers and network are removed even on failure. Only a JSON result is printed;
credentials are not printed. Supply `AOW_ACCEPTANCE_IMAGE` to reuse a prepared test
image. The script deliberately does not validate a host systemd timer inside a
container; scheduler behavior is covered by the native scheduler regression tests.

This establishes network and filesystem isolation on Linux containers. It is not
a claim of physical dual-machine or macOS validation. macOS real-machine acceptance
is deferred; actual host validation can reuse the same endpoint and callback checks.
