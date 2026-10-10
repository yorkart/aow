# Zed ACP integration

This module embeds Zed ACP behavior in the AoW server. AoW calls the public
`AcpService`, host trait and data types exported by `src/lib.rs`. The browser
imports only `frontend/src/features/zed/index.ts`. Backports should change the
implementation behind these boundaries without changing AoW callers.

## Layout

- `src/agent_servers`: explicit adapter installation, installation receipts,
  connections, process transport, client callbacks and connection/session lifecycle.
- `src/acp_thread`: conversation state, event reduction and upstream protocol
  conversions. The content, config option, authentication and prompt capability
  conversions retain the upstream implementation and tests.
- `src/agent_ui/thread_metadata_store`: the SQLite metadata store and queued
  upsert/delete operations, corresponding to Zed's module of the same name.
- `src/db`: the small SQLite connection/migration host adapter.
- `src/project`: ACP Registry discovery, platform selection and adapter installation.
- `src/node_runtime`: Node.js/npm discovery, isolated npm installation and package
  executable resolution.
- `src/settings`: a parsed view of the Zed `agent_servers` configuration.
- `src/facade`: the AoW-owned public contract; no SDK types are exported.
- `crates/server/src/zed`: HTTP and filesystem host adapters.
- `frontend/src/features/zed/agent_ui`: React counterparts of the upstream panel,
  conversation, tool, configuration and elicitation views.

The ACP panel follows Terminal in the right sidebar and reads only locally
registered sessions for the current workspace. Opening or refreshing this list
does not connect to agents or call `session/list`. Like Zed's metadata archive,
it includes conversations created/opened in AoW and explicitly imported history.
The separate **Import ACP sessions** dialog connects installed agents, checks
listing support, paginates their history and lets the user select agents to import.
Canceling discovery saves nothing. Confirmation saves only metadata; opening an
imported record loads its conversation. Import deduplicates by agent, workspace
and remote session ID, preserving existing transcripts and active prompts.
Records without a matching workspace are excluded. The
panel's `+` menu lists installed, supported agents. Selecting one opens a new
central conversation tab; selecting history loads it into a central tab. Reopening a
session selects its existing tab; closing a tab closes only the view, leaving
the session and any active prompt on the server. Tabs with an existing session
restore after a browser reload without sending prompts or creating another
session. Moving a connecting tab between view hosts reuses its pending request.
New tab IDs use random bytes available on LAN HTTP origins. Composer drafts are
kept in memory by workspace and tab ID, independent of portal mounts and title
updates. Closing/deleting the tab discards its draft. A successful prompt clears
only the submitted draft, including when its view moved while the request was
pending; newer input is preserved.
Conversations run in adapter child processes owned by the server. There is no
ACP daemon, PTY or terminald dependency.

## Configuration and storage

AoW Settings owns `<active configuration directory>/zed/settings.json`. It is
JSONC with comments and trailing commas. The root contains `agent_servers` with
Zed's `registry` and `custom` entries. AoW saves the original text with an
optimistic revision check, so comments and unknown fields survive edits. Registry
and preference controls use structural JSONC edits rather than JSON serialization.
The file participates in AoW's existing configuration Git history.

Adapter downloads and conversation history live under `<state directory>/zed`.
They are runtime state, not configuration. Session metadata and the offline
conversation cache use `zed/threads.sqlite`, with `sidebar_threads` following Zed's
metadata store and `acp_thread_snapshots` retaining AoW's display snapshots.
Streaming updates coalesce on a background SQLite writer; lifecycle boundaries
and server shutdown flush pending writes. Listing history reads metadata only.
Existing `zed/history/*.json` files migrate once, transactionally, and remain
unchanged as backups. See [the persistence map](backport/persistence.md) for the
upstream code correspondence, migration and recovery rules.
Protocol logs remain bounded per-connection memory buffers. Registry npm entries require Node.js 22+ and npm. Discovery follows
AoW's configured execution PATH, resolves its Node launcher symlink, and locates
npm beside the real Node executable before checking PATH. Packages are installed
in versioned ACP cache directories and launched with absolute Node and package
entrypoint paths; `npx` does not need to be on PATH. The upstream npm version
ceiling is retained so npm release-age policy still applies.

Registry discovery reads the public ACP Registry index. Adding an entry in
Settings saves the current JSONC draft and immediately installs the adapter,
without starting an ACP connection or creating a session. Settings reports
installation progress, network retries and retryable failures. Installation
receipts survive server restarts and check that the runtime and cached entrypoint
still exist. Uninstalled Registry entries are unavailable for new sessions until
installation completes. Entries added through raw JSONC can be installed from
the configured-agent list. Custom commands do not require Registry installation.

Session tabs show connection progress and conversations, with protocol logs in
the overflow menu. History navigation lives in the sidebar; external discovery runs only in the import dialog.
Authentication controls appear only for the protocol's explicit AuthRequired
state and disappear after authentication, including authentication completed in
another view sharing the connection. Pending authentication supports request-scoped
form and URL elicitations. Authenticating an existing session preserves its unsent
draft and never resends the failed prompt. A new/load/resume request's AuthRequired
state is tracked separately from the snapshot, so an unchanged false snapshot
cannot dismiss login. Explicit successful authentication is shared between views
of that connection; pending login remains mounted until the request finishes.
npm installation has
a five-minute timeout; successful installations reuse the cache. The existing
connect API retains its preparation fallback for older callers; explicit
installation and progress queries are additive facade methods.

Configure a shared download proxy in Settings → Environment, which edits
`~/.config/aow/server.env` on the service host. For example, for a local HTTP proxy:

```dotenv
HTTP_PROXY=http://127.0.0.1:7890
HTTPS_PROXY=http://127.0.0.1:7890
NO_PROXY=localhost,127.0.0.1,::1
```

Use the actual proxy address and omit `export`. Registry HTTP requests and npm
inherit the service environment; each agent does not need its own proxy setting.
Saving only updates the file. The installed server launcher reads proxy variables
on every start, without writing them into macOS system plists. An existing
LaunchDaemon can apply proxy-only changes through a normal service-account
installation/restart, without administrator registration. Other macOS service
settings still use the existing installation/registration flow. When updating
from source, run `just package` before `just install` so the package includes the
latest launcher. Direct binary launches use the parent process environment instead.

Existing connections retain their environment; subsequent connections
use changed configuration. Session defaults are read when opening a session. Settings saves notify the panel
to refresh; server operations read the current file and execution PATH before
reusing a connection. Changing default mode/config values or favorites reuses the
adapter process; these preferences do not alter its launch configuration.

File read/write requests use the AoW filesystem host. Permission and elicitation
requests wait for an explicit UI answer. Terminal execution and terminal login
capabilities are not advertised. Authentication managed by the adapter is supported.
Adapter-owned terminals can stream display-only output through Zed's legacy
`_meta.terminal_output` capability. Terminal metadata, accumulated output and exit
status are saved in session snapshots and rendered with Xterm in expanded tool
cards. This does not enable terminal execution requests or terminal authentication.
Tools without structured content display `rawOutput`, following Zed's fallback.
Session recovery uses the adapter's advertised load/resume capability and never
replays a prompt or tool operation automatically.

## Conversation rendering

The React views follow the ACP branches of the pinned Zed conversation renderer.
User messages remain plain editor text; assistant text uses Markdown. Empty text
and unsupported content do not produce message cards. Thinking expansion follows
`agent.thinking_display` (`auto`, `preview`, `always_expanded`, or
`always_collapsed`) in the independent JSONC file. Tools start collapsed;
authorization requests expose the tool and its explicit permission choices.
File changes use an inline Monaco diff. Unknown protocol events remain in the
snapshot and protocol log rather than appearing as raw JSON conversation cards.
Reply controls appear only after a turn completes, including turns ending in a
tool call. Copy includes all assistant content in that turn, excluding thoughts
and tool output, and supports LAN HTTP. Markdown file links open through AoW's
editor using the session cwd and optional line number.

Permission requests merge their tool preview before presenting choices. Plans
and notices are separate from the transcript so they cannot interrupt streamed
Markdown. Notices show severity and plain-text details and can be dismissed;
dismissal survives refresh. Context compactions update one card per ID, retaining
streamed summaries and displaying failure details. Saved snapshots from earlier
versions migrate their activity and compaction entries on load.

The composer stays at the bottom once messages exist and fills the empty
conversation otherwise. Its toolbar renders the adapter's config options,
including grouped selections, favorites and Boolean controls. Config provider
presence suppresses legacy mode controls even when the provider returns an empty
list. Changing a selection also saves its default in JSONC. Advertised slash
commands appear as suggestions; selecting a suggestion does not send a prompt.

See [the behavior map](backport/frontend-behavior.md) for upstream function
correspondence, validation and explicit host/capability differences.

## Backport workflow

The initial source is `zed-industries/zed` at
`dc3fb21676457b84d2233ac4c6bec5cebc698ec3` (2026-10-08), with
`agent-client-protocol = 2.2.0`, `unstable` and `unstable_protocol_v2`.
Application connections negotiate v1, as the upstream baseline does. Shared v1/v2
conversion behavior remains in the imported conversion modules.

`backport/sources.json` records source paths and hashes, destination paths,
extracted responsibilities, host adaptations and the last synchronization.
The four directly imported conversion files currently match upstream byte for
byte and need no patches. Future differences in imported files must be recorded
as small patches under `backport/patches`. Host ports are explicitly marked as
adaptations, not verbatim imports. The manifest also records behavior coverage
and validation limits.

1. Run `node crates/zed/backport/check.mjs /path/to/zed <new-commit>` to verify
   the recorded baseline and compare every mapped source plus SDK dependencies.
2. Review changed source, protocol events, configuration fields, SDK features and
   upstream tests. Keep SDK upgrades separate unless the target behavior requires
   and validates them.
3. Update imported files and reapply their documented patches. Adapt changed
   behavior inside the corresponding local module. Keep host adapters and local
   presentation changes separate from upstream behavior changes.
4. Extend the capability coverage and event fixtures before advertising new
   capabilities. Replay `tests/fixtures/agent.mjs` through the public facade and
   update browser interaction tests.
5. Run the checks below, inspect the diff, and update source hashes, patches,
   coverage and the sync record. Record retained local differences explicitly.

The source map intentionally excludes the editor, LSP, built-in model services,
collaboration, deprecated Extension migration and non-ACP agents. It includes
upstream dependencies in `project` and `settings_content` even though their host
implementations differ in AoW.

## Validation

```sh
cargo test -p aow-zed
node --test frontend/tests/zed.test.mjs
node --test frontend/tests/architecture.test.mjs
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
npm run lint:js
npm run lint:css
npm run lint:html
npm run lint:config
npm run lint:markdown
npm --prefix frontend run typecheck
just build
cargo test --workspace
```

On macOS, set `TMPDIR` to a writable canonical directory for Cargo tests.
When changing service proxy registration, also run the relevant installer tests:

```sh
node --test scripts/tests/server-launcher.test.mjs scripts/tests/launchdaemon-registration.test.mjs scripts/tests/macos-release.test.mjs
npm run lint:shell
npm run lint:python
```

The opt-in live Registry smoke test downloads an adapter into a temporary cache,
uses an isolated profile, and checks initialization plus session creation (or an
explicit authentication requirement). It sends no prompt:

```sh
AOW_ACP_SMOKE_NODE=/absolute/path/to/node \
AOW_ACP_SMOKE_REGISTRY=/path/to/registry.json \
cargo test -p aow-zed --test integration \
  live_registry_adapter_initializes_with_node_only_path -- --ignored --nocapture
```

The default smoke agent is `codex-acp`; set `AOW_ACP_SMOKE_AGENT` for another ID.
