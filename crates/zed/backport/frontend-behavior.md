# ACP frontend behavior map

Baseline: Zed `dc3fb21676457b84d2233ac4c6bec5cebc698ec3` (2026-10-08).
The source hashes in `sources.json` refer to this commit, regardless of the local
Zed checkout's current HEAD. Browser rendering uses React and DOM; behavior ports
stay under `frontend/src/features/zed`, behind its existing public facade.

## Upstream correspondence

Upstream paths below are relative to `crates/agent_ui/src`; local paths are
relative to `frontend/src/features/zed/agent_ui` unless stated otherwise.

| Upstream behavior | Local counterpart | Preserved rules |
| --- | --- | --- |
| `threads_archive_view.rs`, `agent_panel.rs` | `AgentPanel.tsx` | Session search and navigation, compact relative timestamps, agent selection. AoW places the archive in its right sidebar and opens conversations in workspace tabs. |
| `conversation_view.rs::load_thread` | `SessionTab.tsx`, `config_options.tsx` | Config provider presence replaces legacy controls, including an empty advertised provider. New, loaded and resumed sessions preserve this distinction. |
| `conversation_view.rs::handle_auth_required`, `render_auth_required_state`, `authenticate` | `authentication.tsx`, `SessionTab.tsx` | Login appears for typed AuthRequired; supported methods appear in reverse order; pending login can request input; successful login dismisses the controls. No automatic prompt replay. |
| `config_options.rs` | `config_options.tsx` | Current selection in the trigger, grouped choices, favorites ahead of all choices, search at five choices, Boolean switches, automatic default persistence. Unknown config types are hidden. |
| `conversation_view/thread_view.rs::render_message_editor` | `conversation_view/thread_view.tsx` | Empty conversation input fills the view; existing conversations keep the composer below messages. Expand input, config toolbar, usage indicator, send and cancel. |
| `conversation_view/thread_view.rs::render_entry`, `render_message_content`, `entry_is_finalized_turn_end`, `get_agent_message_content` | `conversation_view/thread_view.tsx`, `entry_view_state.ts`, `reply_actions.tsx` | Plain user input, Markdown assistant output, assistant chunk grouping, whitespace-only message suppression. Reply controls appear at completed turn ends, including tool endings, stay hidden while generating or awaiting permission, and navigate to the corresponding user prompt. Copy includes assistant content from the entire turn, excluding thoughts and tool output. |
| `entry_view_state.rs` | `conversation_view/entry_view_state.ts` | Auto/preview/always-expanded/always-collapsed thinking states and manual override; tool expansion is independent and permissions expose content. |
| `conversation_view/thread_view.rs::render_output_content_block`, `render_markdown` | `conversation_view/content.tsx`, `file_links.ts` | Text, image, embedded-resource URI and resource-link rendering. Markdown file URLs, absolute and relative paths use the host callback, with line numbers and the session cwd. Unsupported output content has no fabricated message view. |
| `conversation_view/thread_view.rs::render_tool_call`, `agent_diff.rs`, `acp_thread::ToolCall::content` | `conversation_view/content.tsx`, `entry_view_state.ts` | Collapsed tools, edit/execute/permission cards, generic raw input, raw output fallback when structured content is empty, canceled tools without visible output hidden, inline old/new file diff. |
| `agent_servers::acp::handle_session_notification`, `conversation_view/thread_view.rs::render_terminal_tool_call` | `crates/zed/src/acp_thread/terminals.rs`, `conversation_view/terminal_output.tsx` | Legacy `terminal_info`, `terminal_output` and `terminal_exit` metadata feed per-session display terminals. Output survives metadata patches, collapse/reopen and snapshot reload. Xterm renders ANSI output without an input channel. |
| `conversation_view/elicitation.rs`, `acp_thread::request_tool_call_authorization_with_id` | `conversation_view/elicitation.tsx`, `crates/zed/src/agent_servers/acp/client.rs` | Explicit permission choices and typed form/URL responses. Permission requests upsert tool content before rendering, including request-only diffs and tools without an earlier notification. Pending permission cards remain visible even if the tool was canceled. |
| `acp_thread::replace_plan`, `push_notice`, `dismiss_notice`, `conversation_view/thread_view.rs::render_session_notices` | `crates/zed/src/acp_thread/mod.rs`, `conversation_view/activity.tsx` | Plans and notices live outside transcript entries and cannot split streaming Markdown or thoughts. Notices preserve plain-text titles and descriptions, severity and explicit dismissal. |
| `acp_thread::upsert_context_compaction_update`, `append_context_compaction_summary`, `conversation_view/thread_view.rs::render_context_compaction` | `crates/zed/src/acp_thread/compaction.rs`, `conversation_view/activity.tsx` | One entry per compaction ID, append-only in-progress summary chunks, replacement/null/omitted patch semantics, lifecycle labels and failure details. Cards without details do not offer expansion. |

The backend preserves typed authentication errors in
`crates/zed/src/facade/error.rs` and config provider presence in the session
snapshot. These are additive facade data; the host does not inspect SDK types or
infer authentication from an English error message.

## Host adaptations and boundaries

- `popover.tsx` provides DOM focus, keyboard navigation, outside-click dismissal
  and viewport placement. The host owns theme colors and workspace tab placement.
- `request.ts` preserves typed HTTP errors. AoW polling and serializable snapshots
  replace GPUI entities and subscriptions. Local snapshots and advertised remote
  history merge by agent and remote ID; they never create a session during discovery.
- `workspace_tabs.ts` generates IDs with random bytes supported on LAN HTTP.
  `composer_drafts.ts` stores transient drafts by workspace and tab ID across
  portal remounts, not in component state or persisted history. Closing a tab
  discards its draft; late send responses cannot clear newer input.
- SessionTab tracks request-required authentication independently of snapshot
  state. An unchanged false snapshot is not evidence of successful login. The
  internal API broadcasts successful authentication to views of that connection;
  pending authentication keeps its request inputs mounted through polling.
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
- Editor context/attachment pickers, local prompt queueing, built-in model agents,
  collaboration, ACP terminal execution and terminal authentication are not part
  of this port. Terminal execution capabilities are not advertised; the legacy
  `_meta.terminal_output` display capability is advertised. V2 terminal updates
  are still retained as protocol data, not interpreted as legacy text chunks. Unknown protocol data
  remains available in snapshots/logs without raw JSON cards in the conversation.

## Verification

`frontend/tests/zed.test.mjs` exercises the public facade in Chromium, including
sidebar/tab separation, session deduplication, portal remounts, reload restoration,
history scoping and authentication, config-provider precedence, default/favorite
persistence, thinking transitions, tool expansion/permissions, inline diffs and
slash command selection. Authentication tests cover retained drafts and no prompt
replay, including authentication completed elsewhere on a shared connection.
LAN HTTP tests use a real insecure browser origin. Recovery tests leave the
snapshot's authentication flag false while the resume request fails and while
login is pending. Draft tests cover both portal directions, title updates,
multiple tabs, close/reopen and prompt responses arriving after a move.
Turn tests cover commentary/tools/final text, pending permissions, cancellation,
historical controls during later turns and navigation across intervening tools.
They also check whole-turn copying after tool endings, permission-only diff
previews, canceled tools awaiting approval, notice details and dismissal, and
compaction summary/error transitions. Markdown file navigation retains URL
sanitization, and clipboard tests cover both insecure origins and denied writes.
Tool tests cover ANSI output updates, collapse/reopen, page reload and structured
content precedence over raw output. The stdio fixture also verifies display-output
capability negotiation, chunk accumulation, exit status and persisted snapshots.
Reducer tests cover out-of-band activity during Markdown streaming, permission
upserts, compaction patch semantics and old snapshot migration; stdio tests cover
the resulting snapshots and durable notice dismissal. Shared Markdown and host
tab regressions are exercised by the session snapshot and tab-link suites.

`frontend/tests/tab-links.test.mjs` checks ACP tabs in the actual AoW host.
`crates/zed/tests/integration.rs` covers typed authentication, request-scoped
elicitation, empty versus absent/null config providers and the adapter lifecycle.
It also verifies that composer defaults/favorites reuse the adapter connection
while updated defaults apply to newly created sessions.
These tests use a protocol fixture; they do not establish full live-agent or GPUI
feature parity. Authenticated live Codex and Claude prompts remain unverified.

When backporting, compare the mapped upstream function before changing its local
counterpart, update the corresponding behavioral fixture, then refresh the
manifest hashes. Keep host-specific changes out of imported conversion modules.
