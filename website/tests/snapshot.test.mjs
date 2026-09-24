import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import { snapshotResponse, readOnlyMessage } from "../snapshot/transport.mjs";
import { blobHash, snapshotSourceState } from "../scripts/snapshot-source.mjs";

const snapshot = JSON.parse(
  await readFile(new URL("../snapshot/data.json", import.meta.url), "utf8"),
);
const project = snapshot.responses["/api/aow/projects"][0];
const workspace = project.worktrees[0].path;

test("captured files, history and terminal output agree with the recorded source manifest", () => {
  assert.equal(snapshot.source, "https://github.com/yorkart/aow");
  assert.match(snapshot.revision, /^[a-f0-9]{40}$/);
  const source = snapshotSourceState(snapshot);
  const files = Object.entries(snapshot.responses).filter(([key]) =>
    key.startsWith(`/api/fs/text${workspace}/`),
  );
  assert.ok(files.length > 200);
  for (const [key, data] of files) {
    const path = key.slice(`/api/fs/text${workspace}/`.length);
    assert.equal(blobHash(data.content), source.files[path]?.split(" ")[1], path);
  }
  const history =
    snapshot.responses[
      `/api/git/log?limit=100&repo=${encodeURIComponent(workspace)}`
    ].commits;
  const commits = source.commits;
  assert.deepEqual(
    history.map((commit) => commit.id),
    commits,
  );
  for (const id of commits)
    assert.ok(snapshot.terminal.output.includes(id.slice(0, 7)));
  assert.deepEqual(snapshot.commands, [
    "git log -5 --oneline",
    "git status --short --branch",
  ]);
  for (const command of snapshot.commands)
    assert.ok(snapshot.terminal.output.includes(command));
  assert.doesNotMatch(
    snapshot.terminal.output,
    /AOW_CAPTURE_DONE|\/Users\/|\/private\/|command not found/,
  );
  assert.equal(project.worktrees[0].head, snapshot.revision);
  assert.deepEqual(
    snapshot.responses[`/api/aow/automations?project_id=${project.id}`],
    [],
  );
  for (const [key, value] of Object.entries(snapshot.responses)) {
    if (key.startsWith("/api/aow/agent-sessions?")) assert.deepEqual(value, []);
  }
});

test("recorded reads work through project Pages prefixes without mutating the capture", () => {
  const path = `/api/fs/text${workspace}/README.md`;
  for (const prefix of ["", "/aow/snapshot", "/another-project/snapshot"]) {
    const result = snapshotResponse(snapshot, prefix + path);
    assert.equal(result.status, 200);
    assert.equal(result.data.content, snapshot.responses[path].content);
    result.data.content = "changed locally";
    assert.notEqual(snapshot.responses[path].content, "changed locally");
    const raw = snapshotResponse(
      snapshot,
      prefix + path.replace("/text", "/raw"),
    );
    assert.equal(raw.raw, true);
    assert.equal(raw.data.content, snapshot.responses[path].content);
  }
  assert.equal(
    snapshotResponse(
      snapshot,
      `/api/git/log?repo=${encodeURIComponent(workspace)}&limit=100`,
    ).status,
    200,
  );
  assert.equal(snapshotResponse(snapshot, "/assets/editor.js"), null);
});

test("uncaptured reads and all writes have explicit failures instead of fabricated successes", () => {
  assert.equal(
    snapshotResponse(snapshot, "/api/fs/text/etc/passwd").status,
    404,
  );
  assert.equal(
    snapshotResponse(snapshot, "/api/aow/agent-sessions?agent=missing").status,
    404,
  );
  for (const [method, path] of [
    ["POST", "/api/terminals"],
    ["POST", "/api/git/push"],
    ["POST", "/api/aow/automations"],
    ["POST", "/api/aow/automations/anything/run"],
    ["PATCH", "/api/aow/settings"],
    ["PUT", `/api/fs/file${workspace}/README.md`],
    ["DELETE", "/api/fs/entries"],
  ]) {
    const result = snapshotResponse(
      snapshot,
      "/aow/snapshot" + path,
      method,
      {},
    );
    assert.equal(result.status, 409);
    assert.equal(result.data.message, readOnlyMessage);
  }
  const tracked = snapshotResponse(snapshot, "/api/git/ignored", "POST", {
    root: workspace,
    paths: [`${workspace}/README.md`],
  });
  assert.equal(tracked.status, 200);
  assert.deepEqual(tracked.data, snapshot.responses["POST /api/git/ignored"]);
  assert.equal(
    snapshotResponse(snapshot, "/api/git/ignored", "POST", {
      root: "/unknown",
      paths: [],
    }).status,
    404,
  );
});

test("the real terminal receives captured bytes and stays in observer mode after takeover", async () => {
  const source = await readFile(
    new URL("../snapshot/transport.mjs", import.meta.url),
    "utf8",
  );
  const window = new EventTarget();
  let networkCalls = 0;
  window.fetch = () => {
    networkCalls++;
    throw new Error("Unexpected live backend access");
  };
  class XHR {
    open() {
      networkCalls++;
    }
  }
  class CloseEvent extends Event {
    constructor(type, options) {
      super(type);
      Object.assign(this, options);
    }
  }
  runInNewContext(
    source.replaceAll("export ", "") + "\ninstallSnapshotTransport(snapshot);",
    {
      snapshot,
      window,
      XMLHttpRequest: XHR,
      URL,
      Request,
      Response,
      EventTarget,
      Event,
      CustomEvent,
      MessageEvent,
      CloseEvent,
      DOMException,
      structuredClone,
      setTimeout,
      queueMicrotask,
    },
  );
  const notices = [];
  window.addEventListener("snapshot-notice", (event) =>
    notices.push(event.detail),
  );
  for (const force of [false, true]) {
    const socket = new window.WebSocket(
      `ws://localhost/aow/snapshot/api/terminals/${snapshot.terminal.tabId}/panes/${snapshot.terminal.paneId}/ws?control=v2`,
    );
    const messages = [];
    await new Promise((resolve) => {
      socket.onopen = () =>
        socket.send(JSON.stringify({ type: "claim", force }));
      socket.onmessage = (event) => {
        messages.push(JSON.parse(event.data));
        if (messages.length === 2) resolve();
      };
    });
    assert.equal(messages[0].state, "observing");
    assert.equal(messages[1].restore, snapshot.terminal.output);
    socket.send(new TextEncoder().encode("npm test\r"));
    assert.equal(
      messages.length,
      2,
      "typing must not produce simulated command results",
    );
    socket.close();
  }
  assert.deepEqual(notices, [readOnlyMessage]);
  const response = await window.fetch("/aow/snapshot/api/terminals", {
    method: "POST",
    body: "{}",
  });
  assert.equal(response.status, 409);
  assert.throws(
    () => new XHR().open("PUT", "/aow/snapshot/api/fs/file/tmp/upload"),
    /只读数据快照/,
  );
  assert.equal(networkCalls, 0);
});

test("Pages deep links return to the static entry while ordinary 404s stay 404s", async () => {
  const source = await readFile(
    new URL("../snapshot-route.js", import.meta.url),
    "utf8",
  );
  for (const base of ["/aow/", "/another-project/", "/"]) {
    const route = `${base}snapshot/aow/tabs/file?workspace=%2Fworkspace%2Faow&path=%2Fworkspace%2Faow%2FREADME.md&ui=mobile`;
    let redirected;
    const location = new URL(route, "https://example.com");
    location.replace = (value) => {
      redirected = new URL(value);
    };
    const document = {
      currentScript: { src: `https://example.com${base}snapshot-route.js` },
    };
    runInNewContext(source, { document, location, URL });
    assert.equal(redirected.pathname, `${base}snapshot/`);
    assert.equal(redirected.searchParams.get("route"), route);
    redirected = undefined;
    const missing = new URL(
      `${base}missing/nested/page`,
      "https://example.com",
    );
    missing.replace = location.replace;
    runInNewContext(source, { document, location: missing, URL });
    assert.equal(redirected, undefined);
  }
});
