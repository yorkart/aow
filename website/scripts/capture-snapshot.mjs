// Capture this project's committed repository data through the real AoW server.
// This never reads an existing AoW state directory or any user's Agent sessions.
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  mkdtemp,
  mkdir,
  writeFile,
  rm,
  stat,
  realpath,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { shouldCaptureCommitDiff } from "./capture-policy.mjs";
import { captureSourceState } from "./snapshot-source.mjs";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const require = createRequire(join(root, "frontend/package.json"));
const WebSocket = require("ws");
const temporary = await realpath(
  await mkdtemp(join(tmpdir(), "aow-snapshot-")),
);
const repo = join(temporary, "aow");
const stateDir = join(temporary, "state");
const socket = join(temporary, "terminald.sock");
const children = [];
const git = (...args) =>
  execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  }).trimEnd();
const revision = git("rev-parse", "HEAD");
const branch = git("branch", "--show-current");
const responses = {};
const omittedCommitDiffs = [];
let serverUrl, cookie;
function canonical(path) {
  const url = new URL(path, "http://snapshot");
  url.searchParams.sort();
  return decodeURI(url.pathname) + (url.search ? url.search : "");
}
async function request(path, method = "GET", body) {
  const response = await fetch(serverUrl + path, {
    method,
    headers: { Cookie: cookie || "", "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(30000),
  });
  const data = await response.json();
  if (!response.ok)
    throw new Error(`${method} ${path}: ${JSON.stringify(data)}`);
  return data;
}
async function record(path, transform = (value) => value) {
  const data = transform(await request(path));
  responses[canonical(path)] = data;
  return data;
}
function start(binary, args) {
  const child = spawn(join(root, "target/debug", binary), args, {
    cwd: root,
    stdio: ["ignore", "pipe", "pipe"],
  });
  children.push(child);
  return child;
}
try {
  git("clone", "--quiet", "--shared", "--no-hardlinks", root, repo);
  // A local clone's origin refs do not prove anything was pushed to GitHub.
  git("-C", repo, "remote", "remove", "origin");
  git(
    "-C",
    repo,
    "remote",
    "add",
    "origin",
    "https://github.com/yorkart/aow.git",
  );
  await mkdir(stateDir);
  await writeFile(
    join(stateDir, "pin.md5"),
    createHash("md5").update("123456").digest("hex"),
  );
  await writeFile(
    join(stateDir, "aow-settings.json"),
    JSON.stringify({ version: 1, notes_base: join(temporary, "notes") }),
  );
  start("aow-terminald", ["--socket", socket]);
  for (let count = 0; count < 100; count++) {
    if (
      await stat(socket).then(
        () => true,
        () => false,
      )
    )
      break;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  const server = start("aow-server", [
    "--host",
    "127.0.0.1",
    "--port",
    "0",
    "--state-dir",
    stateDir,
    "--terminald-socket",
    socket,
    "--frontend",
    join(root, "frontend/dist"),
  ]);
  serverUrl = await new Promise((resolve, reject) => {
    let output = "";
    const timer = setTimeout(
      () => reject(new Error("Snapshot server did not start")),
      15000,
    );
    const read = (chunk) => {
      output += chunk;
      const match = /AoW: (http:\/\/127\.0\.0\.1:\d+)/.exec(output);
      if (match) {
        clearTimeout(timer);
        resolve(match[1]);
      }
    };
    server.stdout.on("data", read);
    server.stderr.on("data", read);
    server.once("error", reject);
  });
  const auth = await fetch(serverUrl + "/api/auth/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pin: "123456" }),
  });
  if (!auth.ok) throw new Error("Snapshot authentication failed");
  cookie = auth.headers.get("set-cookie").split(";")[0];
  const project = await request("/api/aow/projects", "POST", {
    path: repo,
    name: "AoW",
    notes_path: join(repo, "docs"),
  });
  const terminal = await request("/api/terminals", "POST", {
    workspace_root: repo,
    cwd: repo,
    name: "Git 提交记录",
    shell: "/bin/sh",
    rows: 30,
    cols: 100,
  });
  const pane = terminal.panes[0];
  const terminalOutput = await new Promise((resolve, reject) => {
    const ws = new WebSocket(
      serverUrl.replace("http:", "ws:") +
        `/api/terminals/${terminal.id}/panes/${pane.id}/ws?control=v2`,
      { headers: { Cookie: cookie } },
    );
    let output = "",
      sent = false;
    const timer = setTimeout(() => {
      ws.close();
      reject(new Error("Terminal recording timed out"));
    }, 15000);
    ws.on("open", () =>
      ws.send(JSON.stringify({ type: "claim", force: false })),
    );
    ws.on("error", reject);
    ws.on("message", (data, binary) => {
      if (binary) {
        output += data.toString();
        if (/\r?\nAOW_CAPTURE_DONE\r?\n/.test(output)) {
          clearTimeout(timer);
          ws.close();
          resolve(output.slice(0, output.lastIndexOf("\r\nAOW_CAPTURE_DONE")));
        }
      } else {
        const control = JSON.parse(data.toString());
        if (control.type === "stream" && !sent) {
          sent = true;
          ws.send(JSON.stringify({ type: "resize", cols: 100, rows: 30 }));
          // These are the only commands submitted to the isolated captured PTY.
          ws.send(Buffer.from("git log --topo-order -5 --oneline\r"));
          setTimeout(
            () => ws.send(Buffer.from("git status --short --branch\r")),
            250,
          );
          setTimeout(
            () => ws.send(Buffer.from("printf '\\nAOW_CAPTURE_DONE\\n'\r")),
            500,
          );
        }
      }
    });
  });
  await record(`/api/aow/projects/${project.id}/avatar`);
  await record("/api/aow/projects", (values) =>
    values.filter((value) => value.id === project.id),
  );
  await record("/api/aow/settings", (value) => ({
    ...value,
    execution_path: [],
    node_addresses: [],
  }));
  await record("/api/aow/agents?refresh=true", (values) =>
    values.map((value) => ({ ...value, env: {}, executable: value.command })),
  );
  for (const path of [
    "/api/aow/pinned-worktrees",
    "/api/aow/pinned-directories",
    "/api/aow/worktree-removals",
    "/api/aow/notification-settings",
    "/api/aow/im/wechat",
    "/api/operations/active",
    "/api/aow/automations/status",
  ])
    await record(path);
  await record(`/api/aow/automations?project_id=${project.id}`);
  await record("/api/terminals");
  await record(`/api/terminals/${terminal.id}`);
  await record("/api/terminals/agents");
  const query = new URLSearchParams({ repo });
  await record(`/api/git/status?${query}`);
  await record(
    `/api/git/repositories?${new URLSearchParams({ root: repo, depth: "5" })}`,
  );
  await record(
    `/api/git/diff?${new URLSearchParams({ repo, staged: "false" })}`,
  );
  const history = await record(`/api/git/log?${query}&limit=100`, (value) => ({
    ...value,
    commits: value.commits.slice(0, 5),
  }));
  for (const commit of history.commits) {
    const cq = new URLSearchParams({ repo, commit: commit.id });
    await record(`/api/git/commit/detail?${cq}`);
    const changes = await record(`/api/git/commit/files?${cq}`);
    for (const file of changes.files) {
      if (!shouldCaptureCommitDiff(file)) {
        omittedCommitDiffs.push({
          commit: commit.id,
          path: file.path,
          reason: "generated-snapshot",
        });
        continue;
      }
      await record(
        `/api/git/commit/diff?${new URLSearchParams({ repo, commit: commit.id, path: file.path, ...(file.original_path ? { original_path: file.original_path } : {}) })}`,
      );
    }
  }
  for (const agent of ["codex", "claude", "traecli"])
    await record(
      `/api/aow/agent-sessions?${new URLSearchParams({ worktree_path: repo, agent })}`,
    );
  const files = git("-C", repo, "ls-files")
    .split("\n")
    .filter((path) =>
      /^(README\.md|Cargo\.toml|docs\/.*\.md|frontend\/(package\.json|src\/.*\.(tsx?|css))|crates\/server\/src\/.*\.rs)$/.test(
        path,
      ),
    );
  const directories = new Set([repo]);
  for (const file of files) {
    let directory = dirname(join(repo, file));
    while (directory.startsWith(repo)) {
      directories.add(directory);
      if (directory === repo) break;
      directory = dirname(directory);
    }
    await record(`/api/fs/text${join(repo, file)}`);
  }
  for (const directory of directories)
    await record(`/api/fs/tree${directory}`, (value) => ({
      ...value,
      entries: value.entries
        .filter(
          (entry) =>
            directories.has(entry.path) ||
            files.includes(entry.path.slice(repo.length + 1)),
        )
        .map((entry) => ({ ...entry, readonly: true, uid: 0, gid: 0 })),
    }));
  responses["POST /api/git/ignored"] = await request(
    "/api/git/ignored",
    "POST",
    {
      root: repo,
      paths: [...directories, ...files.map((file) => join(repo, file))],
    },
  );
  // Keep the unedited output; remove only the recorder's sentinel command.
  const output = terminalOutput.replace(
    /printf '\\nAOW_CAPTURE_DONE\\n'[^\n]*\n/g,
    "",
  );
  const data = {
    version: 1,
    capturedAt: new Date().toISOString(),
    source: "https://github.com/yorkart/aow",
    revision,
    sourceState: captureSourceState(repo, revision),
    branch,
    omittedCommitDiffs,
    scope:
      "Isolated checkout of this local repository commit; selected tracked files and up to 5 commits. Generated snapshot payload diffs and remote tracking refs are omitted. No personal Agent sessions or scheduled tasks.",
    commands: ["git log --topo-order -5 --oneline", "git status --short --branch"],
    terminal: {
      tabId: terminal.id,
      paneId: pane.id,
      output,
      cols: 100,
      rows: 30,
    },
    responses,
  };
  let serialized = JSON.stringify(data, null, 2)
    .replaceAll(repo, "/workspace/aow")
    .replaceAll(encodeURIComponent(repo), encodeURIComponent("/workspace/aow"))
    .replaceAll(temporary, "/snapshot")
    .replaceAll(encodeURIComponent(temporary), encodeURIComponent("/snapshot"));
  // Keep checkout content and neutral paths, never personal host state.
  serialized = serialized.replaceAll(
    process.env.HOME || "\u0000",
    "/home/snapshot",
  );
  const out = join(root, "website/snapshot/data.json");
  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, serialized + "\n");
  console.log(
    `Captured ${Object.keys(responses).length} responses and a real PTY recording at ${revision.slice(0, 7)} (${files.length} files).`,
  );
} finally {
  for (const child of children.reverse()) child.kill("SIGTERM");
  await Promise.all(
    children.map((child) =>
      child.exitCode !== null || child.signalCode !== null
        ? undefined
        : new Promise((resolve) => child.once("exit", resolve)),
    ),
  );
  await rm(temporary, { recursive: true, force: true });
}
