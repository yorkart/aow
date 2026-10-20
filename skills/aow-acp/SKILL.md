---
name: aow-acp
description: Dispatch tasks to durable asynchronous AoW ACP sessions using aow-cli acp. Use when the user asks to create an ACP subagent, delegate through aow-acp, inspect ACP transcripts, or submit follow-up tasks.
---

# AoW ACP

Use `aow-cli acp` to create or continue durable ACP threads when the user requests dispatch. Keep dispatch instructions in the caller's context and send only the assigned task. Do not substitute terminal Agent commands or the host's native subagent tools.

## Requirements and discovery

- Run as the same OS user on the AoW host with `aow-cli` on PATH and the AoW server running for the same state directory. ACP commands use the local Unix socket, require no browser login or Cookie, and do not require terminald.
- Configure and install the adapter in Settings → ACP, including any required authentication. `aow-cli acp agents` lists configured adapters and availability.
- Discover project IDs and `repo_path` using `aow-cli project list` or `aow-cli project get PROJECT-ID`. Use Git separately to discover existing worktrees. `--cwd` must be an existing repository/worktree root belonging to `--project-id`. Do not prepare a new worktree unless asked.
- `--agent-id` selects an ACP adapter, distinct from a Settings → Agents terminal profile. Pass it only for an explicitly selected adapter; otherwise omit it. The server uses ACP's `default_agent_id`, then the first entry in `aow-cli acp agents` (ordered by adapter ID) when no default is set. Do not invent a default or silently skip unavailable adapters.

## Caller session and parent

Every `create` requires the caller's native session ID through `--session-id` or `AOW_SESSION_ID`. Never use the new child's ID or an adapter configuration ID as the caller ID. For an ACP caller, also pass `--acp-connection-id` or inherit `AOW_ACP_CONNECTION_ID` to scope native session lookup. Dispatch metadata must not change the task text.

Use explicit `--parent terminal:PANE-ID` or `--parent acp:THREAD-ID` to record the creating instance; otherwise AoW inherits `AOW_INSTANCE`. `--no-parent` deliberately creates a root and is required outside an AoW caller. The caller session ID is still required. `--parent` and `--no-parent` are mutually exclusive. Do not use obsolete `--parent-pane-id` or `AOW_PANE_ID`.

## Workflow and commands

1. Determine whether the user needs a new session, a follow-up in an existing thread, or inspection.
2. Identify the project, existing worktree, caller session, and any explicitly selected adapter. Use the default by omitting `--agent-id`.
3. Submit only the user's assigned task. Use `--task-file` for long or multiline prompts.
4. Retain the returned `thread_id`, including when startup fails, and reuse it for inspection or follow-up.
5. Report startup and acceptance separately from task completion.

Discover adapters and create a thread (omit `--session-id` if `AOW_SESSION_ID` is set):

```bash
aow-cli project list
aow-cli acp agents
aow-cli acp create --session-id CALLER-NATIVE-SESSION-ID \
  --project-id PROJECT-ID --cwd /path/to/worktree --task 'User task'
```

Select an adapter and record an ACP parent:

```bash
aow-cli acp create --session-id CALLER-NATIVE-SESSION-ID \
  --project-id PROJECT-ID --cwd /path/to/worktree \
  --agent-id ADAPTER-ID --parent acp:PARENT-THREAD-ID --task-file task.md
```

Create without work, inspect, submit a follow-up, and trace ancestry:

```bash
aow-cli acp create --session-id CALLER-NATIVE-SESSION-ID \
  --project-id PROJECT-ID --cwd /path/to/worktree
aow-cli acp list
aow-cli acp get --thread-id THREAD-ID
aow-cli acp submit --thread-id THREAD-ID --task 'Follow-up task'
aow-cli acp submit --thread-id THREAD-ID --task-file - < task.md
aow-cli instance get acp:THREAD-ID
```

`--task` and `--task-file` are mutually exclusive. `-` reads stdin. Tasks must be nonempty UTF-8 text, at most 128 KiB, without control characters except newline, carriage return, and tab. `create` and `submit` accept `--timeout` in seconds (1–600, default 120) for readiness and submission, not task completion. Use global `--state-dir PATH` before the subcommand for a nondefault server state directory.

## Asynchronous results, inspection, and errors

`create` starts a durable ACP thread and optionally accepts its initial prompt. `submit` resumes a disconnected session if necessary and returns after task acceptance. Busy sessions and sessions controlled by a browser reject submission. Threads remain available after tasks finish.

Creation/submission responses retain `thread_id`, session metadata and parent, plus `phase`, `task_submitted`, and `startup_error`. `phase: ready` means startup completed, and `task_submitted: true` means acceptance; neither proves task completion or success. A startup timeout may return `phase: starting` while initialization continues.

Unlike terminal Agent inspection, `acp get` returns a snapshot including transcript and pending permissions. Inspect that snapshot when the user asks for progress or results, and report only what the transcript and session state establish. `acp list` includes all saved sessions, including browser-created ones. There is no automatic final-answer delivery into the caller's conversation. Users can also open the ACP thread in AoW to view replies and handle permissions.

On startup failure or timeout, report and inspect the returned thread ID before creating another. Do not blindly retry ambiguous submissions: acceptance may still be in progress. Respect browser control and busy errors; arrange release of control before submitting again. Never claim the assigned task completed merely because creation or submission succeeded.
