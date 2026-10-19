# AoW ACP integration

This module embeds Zed ACP behavior in the AoW server. AoW calls the public
`AcpService`, host trait and data types exported by `src/lib.rs`. The browser
imports only `frontend/src/features/acp/index.ts`. Backports should change the
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
- `crates/server/src/acp`: HTTP and filesystem host adapters.
- `frontend/src/features/acp/agent_ui`: React counterparts of the upstream panel,
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
Created sessions have stable `/aow/tabs/acp/<thread_id>`
links through AoW's existing tab router. Links also open in a fresh browser and
reuse an existing floated view when available. The sidebar uses shared AoW panel
styles. ACP and Conversation details reuse `components/conversation` for the
fixed header, transcript, user/agent message frames and right-side turn navigation.
ACP groups its existing entries into user turns through a local presentation
adapter, retaining its own process renderer and live composer below the transcript.
New tab IDs use random bytes available on LAN HTTP origins. Composer drafts are
kept in memory by workspace and tab ID, independent of portal mounts and title
updates. Closing/deleting the tab discards its draft. A successful prompt clears
only the submitted draft, including when its view moved while the request was
pending; newer input is preserved.
Conversations run in adapter child processes owned by the server. There is no
ACP daemon, PTY or terminald dependency.

## Forking sessions

The conversation's **分叉会话** menu action is available when the connected agent
advertises `agentCapabilities.sessionCapabilities.fork`. This is ACP's optional,
unstable `session/fork` method. Only the controlling view can fork an idle session;
active turns and recovery must finish first. Mobile views open the resulting
conversation through the same action.

`POST /api/acp/sessions/<thread_id>/fork` requires the source session's control
token and returns the new snapshot (`?view=overview` selects the browser view).
AoW sends the source's native `sessionId`, workspace `cwd` and `mcpServers: []`.
The new local thread has its own native session identity and records the source
as its parent. Source history and composer drafts remain unchanged. Fork mode and
configuration come from the agent, without applying new-session defaults.

Fork responses need not replay history. For agents supporting `session/load`,
AoW loads the new native session through its existing recovery machinery; other
agents retain the known source transcript locally. The new native identity is
persisted before replay so a failed load can be retried. An abandoned HTTP caller
does not cancel an already admitted fork RPC; the resulting session remains in
the local session list. Forking does not create a Git worktree.

## Configuration and storage

AoW Settings owns `<active configuration directory>/acp/settings.json`. It is
JSONC with comments and trailing commas. The root contains `agent_servers` with
Zed's `registry` and `custom` entries. AoW saves the original text with an
optimistic revision check, so comments and unknown fields survive edits. Registry
and preference controls use structural JSONC edits rather than JSON serialization.
The file participates in AoW's existing configuration Git history.

AoW reads this configuration path directly. Prepare the file at the new path
before upgrading; startup leaves configuration under the former directory alone.

Adapter downloads and conversation history live under `<state directory>/acp`.
AoW initializes this directory directly, without importing former state or
rewriting installation receipts. An empty directory starts with no local history
or installed Registry adapters; install configured Registry agents in Settings.

They are runtime state, not configuration. Session metadata and the offline
conversation cache use `acp/threads.sqlite`, with `sidebar_threads` following Zed's
metadata store and `acp_thread_snapshots` retaining AoW's display snapshots.
Streaming updates queue session references; the background SQLite writer captures
the latest body once per one-second checkpoint. Lifecycle boundaries and server
shutdown flush pending writes. Listing history reads metadata only. Browser
polling uses bounded generation/revision cursors. The browser loads turn overviews
(prompts, conclusions, outcomes and process counts), then fetches a turn's process
only while expanded. Open processes receive text/terminal appends; collapse aborts
requests and releases detail data and mounted terminal/editor resources. Initial
reads and resynchronization preserve this overview/detail boundary.
Existing `acp/history/*.json` files are ignored and left untouched. See
[the persistence map](backport/persistence.md) for the upstream code correspondence,
storage and recovery rules.
AoW API responses use `thread_id` for the local conversation and nullable
`session_id` for the Agent's protocol identity. New conversations are persisted
before connecting: `POST /api/acp/sessions` prepares the record, then the input
owner starts it through the existing `resume` action. Failures and retries retain
the same thread. Links use `/aow/tabs/acp/<thread_id>`; metadata lookup supplies
the workspace without a URL query. Snapshots use these field names without
aliases; saved browser tabs require `thread_id` and records without it are ignored.

Protocol logs use a service-wide memory store: at most 10,000 records and 8 MiB
of serialized records in total, 500 records per connection, and 65,536 characters
per message. Oldest records are evicted first. At most 64 ended or failed
connections are retained; active connections keep their correlation state when
their old records are evicted. Every record includes `connection_id`, `thread_id`,
`session_id` and `scope`. Session log queries collect that thread's events across
its connections, including failed initialization attempts, plus shared connection
events such as initialization and authentication. Shared records retain a null
`thread_id`. Request IDs are correlated by direction and connection; protocol
session IDs correlate notifications. URL elicitation completions retain the
originating scope through `elicitationId`, including after the request response.
Pending request and URL elicitation correlations each retain at most 1,024 entries
per live connection and are cleared when it ends. Deleting a thread removes its
logs and correlations while preserving other threads on shared connections.
Unattributed stderr stays in connection logs. Logs are not persisted and may be
evicted before a later query.

The protocol log view ports Zed's `acp_tools` row layout: direction arrows,
method names, request IDs and notification badges. JSON payloads start as one
highlighted line with horizontal scrolling; clicking the header or preview shows
indented JSON. Responses resolve their method through the opposite request
direction within the same connection. Expanded rows show the formatted payload.
Refresh reads the current thread's logs, copying includes
their correlation fields, and Clear Messages hides records only in the open view.

Registry npm entries require Node.js 22+ and npm. Discovery follows
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

`AcpHost::turn_completed` receives each successfully completed live prompt once,
after its result has been applied and outside the thread-store lock. It includes
the stable local thread ID, prompt ID and assistant-only conclusion. Cancellation,
errors, incomplete stop reasons and history replay do not produce callbacks. The
AoW host forwards completion to the shared task notification dispatcher; its
existing page/IM settings control delivery, and ACP links use the local thread ID.
Notification usage is derived from consecutive cumulative prompt-response counters.
The live response baseline resets when restoring a session, which may have
progressed elsewhere. Missing/reset counters and context-window updates are
never displayed as this turn's token count.

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
streamed summaries and displaying failure details. Cached transcripts retain
their stored entry structure without conversion on load.

The composer stays at the bottom once messages exist and fills the empty
conversation otherwise. Its toolbar renders the adapter's config options,
including grouped selections, favorites and Boolean controls. Config provider
presence suppresses legacy mode controls even when the provider returns an empty
list. Changing a selection also saves its default in JSONC. Advertised slash
commands appear as suggestions; selecting a suggestion does not send a prompt.

### Mid-turn steering

AoW supports the optional `_session/steering` extension when the agent advertises
`initialize._meta.steering.supported: true`. Initialization metadata is exposed
as `ConnectionInfo.extensions`; no agent-name allowlist is used. The `steer`
session action accepts the same content as `prompt`, requires session control,
and leaves cancellation and pending approvals independent. The composer offers
“追加指令” while working and retains a separate stop button.

Delivery is recorded inside the current turn, with pending, accepted, failed,
or unknown status. Acceptance does not assert that the model has acted on the
input. AoW requests `_meta.steering.idleBehavior: "promptRequired"`; when honored,
unconsumed input starts an ordinary owned prompt after the previous one settles.
Cancellation prevents that fallback. `startedNewTurn` means the adapter already
consumed the input and must never trigger a resend. The old prompt's response
cannot finish the new turn. A compatibility reader recognizes codex-acp's
`session_info_update._meta.codex.threadStatus`; idle clears activity but records
an unknown outcome, since it does not prove successful completion. Agents that
start detached turns without a lifecycle signal remain visibly untracked until
they report a state or the session is disconnected; silent output is never used
as a completion heuristic. Unknown delivery is retained and never auto-retried.

This extension is independent of the proposed standard `session/inject` API.
Protocol fixtures cover injection, echo suppression, idle races, cancellation,
capability negotiation, and detached turn ownership.

Steered turns keep their agent messages and delivery cards visible as dialogue,
including earlier answers in the same turn; tools and thinking remain in the
collapsible process. Overview snapshots carry this dialogue in `messages`;
overview patches use `messages: { kind: "delta", entries: [...] }` for indexed
replacements and text suffixes, or `kind: "reset"` for a new dialogue baseline.
Omitted `messages` retains the existing dialogue. Revision gaps and visibility
changes that invalidate message positions require a full snapshot. The cache
retains message positions rather than historical bodies. This does not change
conservative final-answer inference or completion callbacks.
The browser joins text fragments with the same message identity
across steering cards, so a submitted instruction cannot split Markdown in an
active reply. Saved entry order and synchronization cursors are unchanged, and
the same presentation applies to existing saved sessions.

History loading reconciles ordered input blocks and the verified response/tool
transcript, merging response chunks only with matching message identities.
This preserves delivery cards and turn outcomes when native replay places an
input after the response that was streaming at submission time. Repeated text
alone never identifies uncertain delivery; ambiguous or changed history keeps
only outcomes supported by an identical prefix. Failed or unconfirmed inputs
absent from a verified replay remain local delivery cards.

See [the behavior map](backport/frontend-behavior.md) for upstream function
correspondence, validation and explicit host/capability differences.

## Backport workflow

AoW uses ACP names for packages, imports, UI classes and browser events. Upstream
provenance remains in this workflow and `backport/`. The facade and module layout
remain the backport boundary.

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

1. Run `node crates/acp/backport/check.mjs /path/to/zed <new-commit>` to verify
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
cargo test -p aow-acp
node --test frontend/tests/acp.test.mjs
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
cargo test -p aow-acp --test integration \
  live_registry_adapter_initializes_with_node_only_path -- --ignored --nocapture
```

The default smoke agent is `codex-acp`; set `AOW_ACP_SMOKE_AGENT` for another ID.

An additional opt-in test uses an installed, authenticated codex-acp. It performs
two inference turns to verify inherited context, source isolation and restart
recovery. Use a dedicated `CODEX_HOME` when isolating the test from normal history:

```sh
AOW_ACP_FORK_COMMAND=/absolute/path/to/codex-acp \
cargo test -p aow-acp --test fork \
  live_codex_acp_fork_preserves_context_and_resumes -- --ignored --nocapture
```
