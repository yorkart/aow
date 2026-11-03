# ACP frontend behavior map

Baseline: Zed `dc3fb21676457b84d2233ac4c6bec5cebc698ec3` (2026-10-08).
The source hashes in `sources.json` refer to this commit, regardless of the local
Zed checkout's current HEAD. Browser rendering uses React and DOM; behavior ports
stay under `frontend/src/features/acp`, behind its existing public facade.

## Upstream correspondence

Upstream paths below are relative to `crates/agent_ui/src`; local paths are
relative to `frontend/src/features/acp/agent_ui` unless stated otherwise.

| Upstream behavior | Local counterpart | Preserved rules |
| --- | --- | --- |
| `threads_archive_view.rs::update_items`, `thread_metadata_store.rs`, `agent_panel.rs` | `AgentPanel.tsx`, `crates/acp/src/acp_thread/history.rs` | Search/navigation read only registered local history; no agent connection or `session/list` on sidebar refresh. AoW places the archive in its right sidebar and opens conversations in workspace tabs. |
| `thread_import.rs::refresh`, `collect_all_sessions`, `collect_importable_threads`, `import_threads` | `thread_import.tsx`, `crates/acp/src/agent_servers/imports.rs` | Explicit dialog discovers paginated external sessions, selects agents and saves metadata only after confirmation. Missing work directories and duplicate sessions are excluded. Opening an imported record loads it lazily; failed loading retains metadata for retry. |
| `conversation_view.rs::load_thread` | `SessionTab.tsx`, `config_options.tsx` | Config provider presence replaces legacy controls, including an empty advertised provider. New, loaded and resumed sessions preserve this distinction. |
| `conversation_view.rs::handle_auth_required`, `render_auth_required_state`, `authenticate` | `authentication.tsx`, `SessionTab.tsx` | Login appears for typed AuthRequired; supported methods appear in reverse order; pending login can request input; successful login dismisses the controls. No automatic prompt replay. |
| `config_options.rs` | `config_options.tsx` | Current selection in the trigger, grouped choices, favorites ahead of all choices, search at five choices, Boolean switches, automatic default persistence. Unknown config types are hidden. |
| `conversation_view/thread_view.rs::render_message_editor` | `conversation_view/thread_view.tsx` | Empty conversation input fills the view; existing conversations keep the composer below messages. Expand input, config toolbar, usage indicator, send and cancel. |
| `conversation_view/thread_view.rs::render_entry`, `render_message_content`, `entry_is_finalized_turn_end`, `render_thread_controls`, `get_agent_message_content` | `conversation_view/thread_view.tsx`, `entry_view_state.ts`, `reply_actions.tsx` | Plain user input, Markdown assistant output, assistant chunk grouping, whitespace-only message suppression. Reply controls appear at completed turn ends, including tool endings, stay hidden while generating or awaiting permission, and navigate to the corresponding user prompt. Copy includes assistant content from the entire turn, excluding thoughts and tool output. Only the bottom footer offers Scroll to Top; navigation remains available without copyable assistant content. |
| `entry_view_state.rs` | `conversation_view/entry_view_state.ts` | Auto/preview/always-expanded/always-collapsed thinking states and manual override; tool expansion is independent and permissions expose content. |
| `acp_thread::can_merge_message_chunks`, `push_assistant_content_block_with_message_id`, `push_user_content_block_with_protocol_id` | `crates/acp/src/acp_thread/mod.rs`, `crates/acp/src/facade/model.rs` | Protocol message IDs survive snapshot persistence separately from local entry IDs. Adjacent text chunks with different known IDs stay in separate blocks; equal or missing IDs retain streaming concatenation, adopting the first known ID. Assistant blocks render separately within the same response, including automatic follow-ups before `session/prompt` completes. |
| `conversation_view/thread_view.rs::render_output_content_block`, `render_markdown` | `conversation_view/content.tsx`, `markdown.tsx`, `file_links.ts` | Text, image, embedded-resource URI and resource-link rendering. Markdown file URLs, absolute and relative paths use the host callback, with line numbers and the session cwd. Unsupported output content has no fabricated message view. |
| `conversation_view/thread_view.rs::render_tool_call`, `agent_diff.rs`, `acp_thread::ToolCall::content` | `conversation_view/tool_call.tsx`, `content.tsx`, `entry_view_state.ts` | Collapsed tools, edit/execute/permission cards, generic raw input, raw output fallback when structured content is empty, canceled tools without visible output hidden, inline old/new file diff. |
| `agent_servers::acp::handle_session_notification`, `conversation_view/thread_view.rs::render_terminal_tool_call` | `crates/acp/src/acp_thread/terminals.rs`, `conversation_view/terminal_output.tsx` | Legacy `terminal_info`, `terminal_output` and `terminal_exit` metadata feed per-session display terminals. Output survives metadata patches, collapse/reopen and snapshot reload. Xterm renders ANSI output without an input channel. |
| `conversation_view/elicitation.rs`, `acp_thread::request_tool_call_authorization_with_id` | `conversation_view/elicitation.tsx`, `crates/acp/src/agent_servers/acp/client.rs` | Explicit permission choices and typed form/URL responses. Permission requests upsert tool content before rendering, including request-only diffs and tools without an earlier notification. Pending permission cards remain visible even if the tool was canceled. |
| `acp_thread::replace_plan`, `push_notice`, `dismiss_notice`, `conversation_view/thread_view.rs::render_session_notices` | `crates/acp/src/acp_thread/mod.rs`, `conversation_view/activity.tsx` | Plans and notices live outside transcript entries and cannot split streaming Markdown or thoughts. Notices preserve plain-text titles and descriptions, severity and explicit dismissal. |
| `conversation_view/thread_view.rs::{render_activity_bar, render_plan_summary, render_plan_entries}`, `conversation_view.rs::plan_summary_label`, `acp_thread::{Plan::stats, clear_plan}` | `conversation_view/plan.tsx`, `thread_view.tsx`, `crates/acp/src/agent_servers/mod.rs` | Initially collapsed plan, current-task summary, completion/cancellation counts, bounded task list and explicit clearing. Activity cards share the message column. |
| `acp_thread::upsert_context_compaction_update`, `append_context_compaction_summary`, `conversation_view/thread_view.rs::render_context_compaction` | `crates/acp/src/acp_thread/compaction.rs`, `conversation_view/activity.tsx` | One entry per compaction ID, replacement/null/omitted patch semantics, lifecycle labels and failure details. SDK 3.4.0/schema 1.12.0 extends the pinned source behavior: first-seen chunks create in-progress entries and later chunks append in receive order regardless of status, preserving entry identity and compaction metadata. Cards without details do not offer expansion. |

The backend preserves typed authentication errors in
`crates/acp/src/facade/error.rs` and config provider presence in the session
snapshot. These are additive facade data; the host does not inspect SDK types or
infer authentication from an English error message.

## Tool presentation and typography

`conversation_view/reply_action_icons.tsx` preserves the pinned
`assets/icons/{copy,user_arrow_up,arrow_up}.svg` paths and stroke width, rendered
at Zed's Small (14px) size with the host theme color. Reply actions follow Zed's
order and English tooltips: Copy This Agent Response, Scroll to User Message,
Scroll to Top. Copy retains its icon after activation. User navigation targets
the turn's prompt, falling back to the most recent user entry or the conversation
bottom if no user entry exists; top navigation scrolls only the transcript.

`conversation_view/tool_call.tsx` follows `render_tool_call`,
`render_tool_call_label` and `render_terminal_tool_call`. Ordinary tool labels keep
the kind icon and Markdown title; a single location navigates independently of
expansion. Execute titles are literal, wrapping code with a separate copy action.
`ui/command.tsx` adds Bash syntax colors through Refractor's core and Bash grammar,
rendering tokens as React text/spans without interpreting command text as HTML.
Highlighting is limited to execute/terminal cards; other tool labels and terminal
output keep their existing renderers. Tokenization is memoized by command text,
with a literal fallback for errors or commands over 32,768 characters; copying uses the
unchanged source text.
`ui/terminal_tool_header.tsx` separates directory, disclosure and running/failure
state from the command. Terminal-bearing calls render terminal cards directly.
Expanded ordinary tools show raw input before output; permission previews show
content first with separately collapsible raw input. Execute, edit and image
calls suppress raw input. Rejected calls hide output. Permission actions reuse
the existing response path without duplicating the preview's title or input.

`conversation_view/markdown.tsx` uses the shared sanitized Markdown renderer with
Agent soft breaks and ACP-local copy/wrap controls. Wrap preferences survive
streaming replacements. Ordinary user/agent messages use the shared Conversation
prose typography, headings, quotes, code blocks and tables. Markdown inside tools
and thinking retains the ACP process styles. Inline diffs use content-sized Monaco views without a
line-number gutter. Terminal output uses ANSI-aware Xterm, grows with content,
and scrolls when it reaches the embedded view limit.

The shared Conversation theme supplies the host type scale: session body 13px
with 1.8 line height, code/tool labels 12px, and metadata 11px. ACP retains Zed's
UI versus buffer font roles, compact command line height and smaller metadata,
while using this host scale rather than Zed's default 16px UI base. Commands,
Markdown code, terminal output and diffs share the monospace role. The session
Markdown and ACP body both consume the same base-size and line-height variables.

The current display-terminal snapshot has no execution control, elapsed-time or
truncation metadata; those controls/indicators are not synthesized. Native Zed
project actions (such as discarding a partially applied edit) remain outside the
ACP host adapter. Generic ACP content, diff previews and file navigation use the
existing public interface.

## Plan and request cards

`conversation_view/plan.tsx` follows the pinned Zed plan state and rendering:
the plan starts collapsed, a collapsed active plan shows `Current:` and the
remaining count, and expansion shows `Plan` with the task summary. In-progress
tasks count as remaining; completed and canceled tasks follow `plan_summary_label`.
Rows use Markdown, status icons, completed strikethrough and a 160px scroll limit.
The header toggles expansion, while Clear Plan dismisses the plan independently.
Expansion survives streamed updates within a view and resets for another thread.
AoW persists clearing in its plan snapshot through the owner-controlled
`clear_plan` action; this does not cancel execution. The next agent plan update
can show a plan again.

`conversation_view/elicitation.tsx` follows `ElicitationCard::render` and
`render_form`: a compact requester/status header, message and labeled inputs,
single-choice radio cards, multi-choice checkbox cards, and separate response
buttons. Schema defaults and typed responses are preserved; receiving or choosing
an option does not submit an answer. `render_permission_buttons_flat` supplies
the vertical permission choices and allow-once/allow-always/reject icon semantics.
Tool permission options use their own full-width column rather than the shared
wrapping action bar; long labels wrap inside their row. Like Zed's flat tool
permissions, only agent-provided options are shown, without an extra Cancel
choice. This is distinct from Zed's generic v2 permission card and elicitation
forms, which have a separate Cancel response.
Embedded permissions reuse the tool preview; standalone requests have a matching
header with expandable raw request details.

The card skeleton comes from Zed; fonts, colors, focus/hover states and responsive
spacing use AoW's Conversation theme. Shared content-width and inset variables
keep activity cards aligned with agent output, capped at 900px. A bounded,
scrollable form body keeps the header and response buttons visible in short or
narrow panes. Native DOM inputs supply keyboard navigation and browser validation.

The composer shares the message column's width and responsive insets. Its editor
and toolbar sit inside one rounded container with a compact 120px minimum height.
Empty conversations keep this compact composer at the bottom; explicit expansion
still enlarges the editor without changing its width or discarding the draft.

## Host adaptations and boundaries

- Successful live `session/prompt` responses emit one host completion callback,
  identified by the local thread and prompt IDs. AoW feeds this into its existing
  task notification dispatcher and page/IM preferences. History reads, canceled
  prompts, errors and other stop reasons do not emit completion. ACP notifications
  navigate by `thread_id`, share the unread queue/workspace counts, and clear when
  that ACP conversation becomes active. Closing a popup retains unread state.
  Terminal targets remain distinct even when their IDs match ACP thread IDs.
  Conclusions contain only the completed turn's assistant text. Consecutive ACP
  response counters are converted from cumulative to per-turn usage; context-window
  updates, missing baselines and reset counters cannot supply a per-turn badge.
- `popover.tsx` provides DOM focus, keyboard navigation, outside-click dismissal
  and viewport placement. The history panel reuses AoW's panel header, list-row
  styles and icon buttons. Conversation colors share the host's Conversation
  palette. `components/conversation/ConversationLayout.tsx` owns the shared fixed
  header, transcript scrolling, user/agent frames and right-side turn navigation
  used by both ACP and Conversation details. The ACP-local `conversation_turns.ts`
  adapts existing message groups without translating process content into native
  snapshot activities. Multipart user input stays in one turn; entries without a
  preceding user remain visible ahead of numbered turns. Only the current working
  turn receives a status; ACP does not provide historical per-turn outcomes.
  Tools, thinking, permissions, reply controls and the live composer remain in
  the ACP module. Streaming follows the end until the user scrolls into history;
  returning to latest or sending a prompt enables following again. Header and
  composer stay outside the transcript scroll region; narrow panes hide navigation.
  Following `render_entries` / `render_generating`, the generating indicator
  follows the latest agent output inside the scrolling transcript. It appears
  beneath the agent header before any output arrives, including histories with
  no retained user entry, and disappears when the session stops working.
  `ui/spinner_label.tsx` preserves Zed's Dots frames (one-second cycle) and Sand
  frames (two-second cycle). Generation shows the character animation alone;
  permission/elicitation waits show Sand with `Awaiting Confirmation`. Active
  context compaction suppresses the extra indicator. Reduced-motion preferences
  pause the animation, and its decorative frames stay hidden from screen readers.
- The shared layout owns the page font baseline and control styles. Body line
  height belongs to message content, never the ACP page wrapper. ACP's
  `acp-content` control rules apply only inside its entries, composer, activity,
  authentication and logs; they cannot reach the shared header, message frames or
  navigation. Pending sessions also render through `ConversationLayout`. Optional
  header metadata/actions and the live composer use the existing content slots.
  The shared header uses one compact row: agent icon, title and plain worktree
  directory name with the full path on hover. The former agent/turn-count subtitle
  is omitted; right-side metadata and actions retain their existing behavior.
  Turn navigation, latest-turn and reply-footer jumps scroll immediately without
  animation, keeping the header and composer fixed.
- The host's `tabRoutes/acp.ts` maps persisted `thread_id` values to workspace-free URLs.
  The public `resolveAcpSession` reads metadata without connecting an adapter;
  browser-local draft tab IDs are used only to reveal an existing view, including
  floated tabs. A fresh browser opens the same persisted session. Mobile links
  reuse the conversation renderer through `AcpSessionPage`.
  Mobile's ACP bottom-navigation entry lists registered sessions in the current
  worktree, newest first, with agent icons and completion indicators. Browsing and
  refreshing the list only read local metadata. Selecting a row opens the same
  persisted conversation; the list button and bottom navigation return to the
  list, while browser history and reload retain the session URL.
- Exclusive input control belongs to AoW, in `crates/server/src/acp/control` and
  `frontend/src/features/acp-control`; ownership tokens stay outside the `aow-acp` facade.
  Each mounted tab attaches over an authenticated, same-origin WebSocket and
  claims only a vacant session. The server issues a token scoped to that
  attachment and session. An owner automatically restores disconnected/imported
  history before accepting input; a live or working session is reused. Failures
  expose retry or authentication, without a routine manual resume step.
  Observers poll the same transcript, hide the composer and disable session
  actions; explicit takeover replaces the token and makes the old owner read-only.
  Closing a view, logout or a missed heartbeat releases control without closing
  the agent session. Reconnecting observers stay observers. Portal moves retain
  the attachment and share an in-flight restore; drafts survive ownership changes.
  Session HTTP mutations validate the current token, including tool permission
  replies; deletion of an unowned archive record remains available in the sidebar.
  Creation/replay, close and deletion are serialized, and queued operations recheck
  ownership. Permission replies and cancellation remain available. Operations
  already admitted may finish after takeover; takeover itself never cancels work.
  The unused HTTP connection-wide disconnect/load endpoints are no longer exposed:
  loading goes through import plus an owned session resume, and connection forms
  cannot answer session-scoped permissions. Their Rust facade methods are unchanged.
- `request.ts` preserves typed HTTP errors. The browser requests turn overviews,
  followed by generation/revision-based incremental polling. Process bodies and
  terminal output load only while a process is expanded; open processes receive
  scoped incremental updates. Expired cursors resynchronize within that scope.
  Collapsing a process releases its data and aborts requests, rather than retaining
  a hidden tree or an unbounded cache. Collapsed tool groups also unmount children,
  disposing terminals, editors and their observers. Active processes default open;
  completed ones close, and pending permissions force the relevant process open.
  Content/tool renderers are memoized. Full history virtualization is deferred.
  The SQLite `ThreadMetadataStore` follows
  Zed's metadata/operation boundary. AoW's separate SQLite body cache retains the
  additive `needs_load` placeholder for imports and existing offline transcripts.
  Imports preserve timestamps and existing snapshots. A record is hydrated through
  the existing resume/load facade when opened by a view that acquires control. See the
  [persistence map](persistence.md) for source correspondence and storage behavior.
- AoW scopes import and history to the current workspace. Zed's import modal can
  discover threads across worktrees and initially marks them archived; AoW has one
  workspace history list and no separate archive state. Deduplication uses agent,
  cwd and remote ID, since remote IDs are not globally unique across adapters.
  Unsupported listing, authentication and discovery errors stay in the explicit
  import dialog. Partial import failures can be retried without duplicating or
  replacing already registered records.
- `workspace_tabs.ts` generates IDs with random bytes supported on LAN HTTP.
  `composer_drafts.ts` stores transient drafts by workspace and tab ID across
  portal remounts, not in component state or persisted history. Closing a tab
  discards its draft; late send responses cannot clear newer input.
- `message_editor/` ports clipboard image mentions from `MessageEditor` and
  `MentionSet`. Native DOM editing retains caret position, IME and undo; inline
  `Image` chips expose a hover preview. Text and images are submitted in editor
  order, including image-only prompts. Image input uses the advertised prompt
  capability, with a matching backend check.
- Clipboard images are converted to PNG/Base64 with Zed's 1568-pixel bound and
  5 MiB encoded-image cap (up to eight further resize passes). Browser decoding
  and canvas resampling replace GPUI/image-rs; supported input formats depend on
  the browser. Conversion failures remain removable error chips in AoW.
- AoW serializes selected text and ready image bytes into a private clipboard
  format, preserving mixed content when copying/cutting between browser editors,
  even after the source draft is cleared. This is a host extension: the pinned
  Zed version does not restore pasted-image mention URIs from its clipboard.
  Pending or failed images block copy/cut with an error and keep the selection.
  Paste validates the structured format and rebuilds local chips and text;
  external clipboard HTML is never inserted. Native undo/redo remains available.
- No attachment upload endpoint or file store is added. Base64 content is saved
  in the existing SQLite snapshots; remote `session/load` retains its existing
  agent-history replay semantics. Images in unsent drafts survive portal moves
  but are discarded on tab close or browser reload, just like the existing view
  draft. Rejected HTTP submissions retain their draft. Acceptance clears an
  unchanged document even if a removed image finishes conversion in the meantime;
  edits made while awaiting acceptance remain in the composer.
- The host action route accepts at most 32 MiB of JSON; other routes keep their
  existing limits. Native paste events work on LAN HTTP without the async
  Clipboard API. Matching image echoes are suppressed once per submitted block.
- SessionTab tracks request-required authentication independently of snapshot
  state. An unchanged false snapshot is not evidence of successful login. The
  internal API broadcasts successful authentication to views of that connection;
  pending authentication keeps its request inputs mounted through polling.
  Request-only auth state is scoped to the control token so an earlier owner's
  failed restore cannot redisplay a stale login form after takeover and login elsewhere.
- Markdown uses AoW's shared renderer; file diffs use its Monaco editor. This does
  not reuse the native GPUI editor or guarantee identical typography and layout.
- File links use opt-in host handling without relaxing the shared Markdown URL
  sanitizer. Reply copying tries the Clipboard API and then a selection-based
  fallback for LAN HTTP; a failed copy reports an error in the view.
- Plans and notices use additive snapshot fields. Old history moves activity out
  of transcript entries and merges compaction updates. AoW persists notice
  dismissal across polling and reloads; these remain advisory UI state, not
  replayed agent messages. Older standalone summary chunks remain visible, but
  their discarded compaction IDs cannot be recovered.
- ACP diff previews detach their Monaco models before disposal, so streamed
  content removal and permission transitions do not race the diff worker.
- Choice search uses case-insensitive subsequence matching, not Zed's native
  fuzzy-ranking implementation. Slash completion uses advertised ACP commands.
- Editor file/context and attachment pickers, local prompt queueing, built-in model agents,
  collaboration, ACP terminal execution and terminal authentication are not part
  of this port. Terminal execution capabilities are not advertised; the legacy
  `_meta.terminal_output` display capability is advertised. V2 terminal updates
  are still retained as protocol data, not interpreted as legacy text chunks. Unknown protocol data
  remains available in snapshots/logs without raw JSON cards in the conversation.

## Verification

`frontend/tests/acp.test.mjs` exercises the public facade in Chromium, including
sidebar/tab separation, session deduplication, portal remounts, reload restoration,
local-only history refresh, explicit import and authentication, config-provider precedence, default/favorite
persistence, thinking transitions, tool expansion/permissions, inline diffs and
slash command selection. Authentication tests cover retained drafts and no prompt
replay, including authentication completed elsewhere on a shared connection.
LAN HTTP tests use a real insecure browser origin. Recovery tests leave the
snapshot's authentication flag false while the resume request fails and while
login is pending. Draft tests cover both portal directions, title updates,
multiple tabs, close/reopen and prompt responses arriving after a move.
Shared-layout tests cover fixed header/composer positions, right-side turn
navigation, multipart input, history without a prompt, reading during streaming,
return-to-latest, expanded input, retained drafts/tool expansion through protocol
logs, and narrow-pane overflow. Conversation snapshot tests retain native process
expansion, refresh scroll position, session reset and public-view behavior.
Cross-page browser comparisons check computed fonts, colors, padding, borders,
header/control heights, prose formatting and navigation at 1140, 800, 600 and
390px widths against the real Conversation renderer, with an identical host theme.
Turn tests cover commentary/tools/final text, pending permissions, cancellation,
historical controls during later turns and navigation across intervening tools.
They verify footer action order and English tooltips, bottom-only top navigation,
tool-only navigation without a copy action, missing-user fallback, real transcript
scrolling, fixed header/composer positions, and drafts retained across navigation.
They also check whole-turn copying after tool endings, permission-only diff
previews, canceled tools awaiting approval, notice details and dismissal, and
compaction summary/error transitions. Markdown file navigation retains URL
sanitization, and clipboard tests cover both insecure origins and denied writes.
Tool tests also compare computed session/ACP typography, command wrapping and
literal copying on HTTP, Markdown labels and file navigation, input/output order,
permission preview deduplication, streaming code controls and terminal card states.
Activity-card tests cover default collapse, updates without expansion resets,
completion/cancellation summaries, bounded scrolling, clearing across reloads,
and matching message widths at desktop and narrow viewport sizes. Request tests
cover radio keyboard navigation, typed checkbox responses, explicit submit/decline/
cancel actions, permission choices and visible mobile response buttons.
Tool tests cover ANSI output updates, collapse/reopen, page reload and structured
content precedence over raw output. The stdio fixture also verifies display-output
capability negotiation, chunk accumulation, exit status and persisted snapshots.
Reducer tests cover out-of-band activity during Markdown streaming, permission
upserts and compaction patch semantics; SQLite round trips preserve message blocks
and entry IDs, and stdio tests cover
the resulting snapshots and durable notice dismissal. Shared Markdown and host
tab regressions are exercised by the session snapshot and tab-link suites.

`frontend/tests/tab-links.test.mjs` checks ACP tabs in the AoW frontend served by Vite.
It covers session URLs after creation, fresh-browser login and reload, browser
history navigation, retained drafts, floating views, missing sessions and
removed workspaces without starting an agent for invalid links. ACP routes carry
only `thread_id`; metadata resolves the cwd, and extra workspace queries are
discarded. Saved tab descriptors require `thread_id`; records using only the old
local `sessionId` key are ignored. The Agent's protocol `sessionId` is unchanged.

The protocol log view in `frontend/src/features/acp/acp_tools` ports
`crates/acp_tools/src/acp_tools.rs::{render_message, push_stream_message_for_connection,
collapsed_params_md, expanded_params_md}`. It retains direction colors, method
lookup by request direction, request/type chips, compact previews and expanded
JSON. React text tokens replace Markdown code blocks; compact formatting preserves
string values. AoW keeps its shared Conversation font scale and thread-scoped query,
with formatted payloads in expanded rows. Refresh fetches logs without restarting
the adapter; Clear Messages affects only the open view. Browser tests cover
connection/direction/typed-ID isolation, truncated records, JSON batches, literal
copying, keyboard expansion, refresh, view clearing and narrow widths.

New tabs prepare and persist their thread before any adapter connection, then
the controlling view creates it using the same automatic restore path. Failed
initialization and authentication retain that identity. The thread connection
query keeps connection-scoped information requests interactive during creation.
Protocol logs expose per-thread records plus explicitly labeled shared connection
events; correlation and initialization lifetime are AoW adaptations within the
debug-log boundary, not changes to the ACP wire protocol.
Only the owning view automatically restores a saved session; link resolution
itself remains a read. Multi-context tests cover takeover during live work,
permission gating, stale credentials, observer reconnects, shared imported loads,
authentication across ownership changes and drafts retained through portal moves.
The server control tests exercise real WebSocket claims, takeover notifications,
disconnect/logout release, origin checks and HTTP admission for every session action.
`crates/server/src/tests.rs` and `crates/server/src/base_path/tests.rs` exercise
the production Rust router: direct ACP links return the HTML entry point,
including encoded workspace paths, mobile selection and deployment prefixes.
These checks are necessary because Vite's SPA fallback does not validate the
production server's page-route registrations.
`crates/acp/tests/integration.rs` covers typed authentication, request-scoped
elicitation, empty versus absent/null config providers and the adapter lifecycle.
It also verifies that composer defaults/favorites reuse the adapter connection
while updated defaults apply to newly created sessions. Import tests check that
browsing, canceling and saving metadata never load/create/prompt a conversation;
workspace filtering, repeated pagination cursors, selection, duplicates,
unsupported agents, slow-agent isolation, authentication and retryable errors are
covered in Chromium.
The stdio tests verify durable metadata and timestamps across service restarts,
load-on-open, recovery from failed replay, whole-selection validation and
reimporting an active conversation without replacing it.
These tests use a protocol fixture; they do not establish full live-agent or GPUI
feature parity. Authenticated live Codex and Claude prompts remain unverified.

When backporting, compare the mapped upstream function before changing its local
counterpart, update the corresponding behavioral fixture, then refresh the
manifest hashes. Keep host-specific changes out of imported conversion modules.
