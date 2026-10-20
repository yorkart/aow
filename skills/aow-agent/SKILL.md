---
name: aow-agent
description: Dispatch tasks to independent asynchronous AoW Agent terminal sessions using aow-cli agent. Use when the user asks to create an AoW subagent, delegate through aow-agent, inspect sessions, or submit follow-up tasks.
---

# AoW Agent

Use `aow-cli agent` to create or continue independent AoW Agent terminals when the user requests dispatch. Keep these dispatch instructions in the caller's context; submit only the assigned task to the child. Do not substitute the host's native subagent tools.

## Requirements and discovery

- Run as the same OS user on the AoW host, with `aow-cli` on PATH, the AoW server and terminald running for the same state directory, and the selected Agent installed and logged in. The local Unix socket requires no browser login or Cookie.
- Discover the registered project with `aow-cli project list` or `aow-cli project get PROJECT-ID`. These return IDs and `repo_path`, not worktrees. Use `git -C /repo worktree list --porcelain` when needed. `--cwd` must be an existing repository/worktree root belonging to `--project-id`.
- Prepare worktrees separately only when requested. Dispatch never creates, switches, resets, or removes worktrees or branches.
- Interactive startup supports Codex, TraeCode CLI, Hermes, and Pi. Settings → Agents also lists other types; an unsupported type fails explicitly.
- Pass a configuration ID from Settings → Agents via `--agent-id` only when the user selects one. IDs are case-sensitive and contain 1–80 letters, numbers, `-` or `_`. Omit the flag otherwise: the server uses the saved default Agent, then the first listed Agent if no default is set. Do not hardcode `codex`, skip an unavailable selection, or substitute an ACP adapter ID.
- Ask for missing context only when project, worktree, or caller identity cannot be determined. Never invent IDs or paths.

## Caller session and parent

Every `create` requires the caller's native session ID through `--session-id` or `AOW_SESSION_ID`. This identifies the caller, never the new child. ACP callers also supply `--acp-connection-id` or inherit `AOW_ACP_CONNECTION_ID` to scope the native session lookup. These fields do not alter the task text.

Parent selection uses explicit `--parent terminal:PANE-ID` or `--parent acp:THREAD-ID`, then inherited `AOW_INSTANCE`. Use `--no-parent` only when intentionally creating a root, including calls from outside AoW. Even a root requires the caller session ID. An AoW terminal without inherited identity needs an explicit parent. Do not use the obsolete `--parent-pane-id`, `AOW_PANE_ID`, or `--agent` selection mechanisms.

## Workflow and commands

1. Decide whether the request needs a new terminal, a follow-up in an existing pane, or inspection.
2. For creation, identify the registered project, existing worktree, caller session, and any user-selected profile. Leave `--agent-id` omitted to use the configured default.
3. Submit the user's task without dispatch instructions, creation commands, or unrelated configuration details. Use `--task-file` for multiline or large tasks.
4. For follow-ups, reuse the returned `pane_id`.
5. Report the returned IDs, startup outcome, and submission status, distinguishing acceptance from task completion.

Create a terminal and submit work (omit `--session-id` when `AOW_SESSION_ID` is already set):

```bash
aow-cli project list
aow-cli project get PROJECT-ID
aow-cli agent create --session-id CALLER-NATIVE-SESSION-ID \
  --project-id PROJECT-ID --cwd /path/to/worktree --task 'User task'
```

Select a profile and record an explicit parent:

```bash
aow-cli agent create --session-id CALLER-NATIVE-SESSION-ID \
  --project-id PROJECT-ID --cwd /path/to/worktree \
  --agent-id CONFIG-ID --parent terminal:PARENT-PANE-ID --task-file task.md
```

Create without a task, inspect, or submit a follow-up:

```bash
aow-cli agent create --session-id CALLER-NATIVE-SESSION-ID \
  --project-id PROJECT-ID --cwd /path/to/worktree
aow-cli agent list
aow-cli agent get --pane-id PANE-ID
aow-cli agent submit --pane-id PANE-ID --task 'Follow-up task'
aow-cli agent submit --pane-id PANE-ID --task-file - < task.md
aow-cli instance get terminal:PANE-ID
```

`--task` and `--task-file` are mutually exclusive. `-` reads stdin. Tasks must be nonempty UTF-8 text, at most 128 KiB, without terminal control characters except newline, carriage return, and tab. Creation `--timeout` accepts 1–600 seconds (default 120), covering startup and initial submission, not execution of the task. Use global `--state-dir PATH` before the subcommand if the server uses a nondefault state directory.

## Asynchronous results and errors

Creation starts a hidden 160×48 terminal and waits for input readiness and optional task submission. `submit` writes the task and Enter and returns without waiting for a reply. Terminals and worktrees remain after tasks finish; CLI-created tabs use automatic titles and cannot be renamed.

Responses include `pane_id`, `tab_id`, `agent`, `cwd`, `status`, `phase`, `task_submitted`, `error`, and optional `parent: {kind, id}`. `phase` is `starting`, `ready`, or `failed` and describes initialization. `ready`, a running terminal, or `task_submitted: true` does not establish that the task succeeded or finished.

These commands do not retrieve the final answer or deliver it automatically to the caller. Polling `get` or `list` cannot collect the task result. Report that the task runs asynchronously and direct the user to the project's Terminal view in the right sidebar to observe progress and replies; result collection needs a separate agreed handoff.

On startup failure or timeout, preserve and report returned pane/tab IDs and inspect the pane before creating another. On human control or hosting conflicts, ask the user to release the relevant control in AoW. Never blindly retry an ambiguous submission or claim completion from successful dispatch.
