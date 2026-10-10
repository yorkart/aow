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
| `conversation_view/thread_view.rs::render_entry`, `render_message_content` | `conversation_view/thread_view.tsx`, `entry_view_state.ts` | Plain user input, Markdown assistant output, assistant chunk grouping, whitespace-only message suppression. |
| `entry_view_state.rs` | `conversation_view/entry_view_state.ts` | Auto/preview/always-expanded/always-collapsed thinking states and manual override; tool expansion is independent and permissions expose content. |
| `conversation_view/thread_view.rs::render_output_content_block` | `conversation_view/content.tsx` | Text, image, embedded-resource URI and resource-link rendering; file links navigate through the host callback. Unsupported output content has no fabricated message view. |
| `conversation_view/thread_view.rs::render_tool_call`, `agent_diff.rs` | `conversation_view/content.tsx` | Collapsed tools, edit/execute/permission cards, generic raw input, canceled tools without visible output hidden, inline old/new file diff. |
| `conversation_view/elicitation.rs` | `conversation_view/elicitation.tsx` | Explicit permission choices and typed form/URL responses. |

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
- Choice search uses case-insensitive subsequence matching, not Zed's native
  fuzzy-ranking implementation. Slash completion uses advertised ACP commands.
- Editor context/attachment pickers, local prompt queueing, built-in model agents,
  collaboration, ACP terminal execution and terminal authentication are not part
  of this port. Terminal capabilities are not advertised. Unknown protocol data
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
