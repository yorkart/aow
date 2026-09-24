export const readOnlyMessage =
  "当前为只读数据快照，此操作需要在已安装的 AoW 中执行。";

export function keyFor(url) {
  const parsed = new URL(url, "http://snapshot");
  parsed.searchParams.sort();
  return decodeURI(parsed.pathname) + parsed.search;
}

// Match only recorded reads. Missing data is never replaced with invented results.
export function snapshotResponse(snapshot, requestUrl, method = "GET", body) {
  const url = new URL(requestUrl, "http://snapshot");
  const apiIndex = url.pathname.indexOf("/api/");
  if (apiIndex < 0) return null;
  url.pathname = url.pathname.slice(apiIndex);
  const path = decodeURI(url.pathname);
  if (method === "POST" && path === "/api/git/ignored") {
    const recorded = snapshot.responses["POST /api/git/ignored"];
    const knownPath = (path) =>
      Object.hasOwn(snapshot.responses, `/api/fs/text${path}`) ||
      Object.hasOwn(snapshot.responses, `/api/fs/tree${path}`);
    if (
      recorded &&
      body?.root === recorded.repository &&
      Array.isArray(body.paths) &&
      body.paths.every(knownPath)
    ) {
      return {
        status: 200,
        data: {
          ...recorded,
          ignored: recorded.ignored.filter((path) => body.paths.includes(path)),
        },
      };
    }
    return { status: 404, data: { message: "这项数据未收录在当前快照中。" } };
  }
  if (method !== "GET" && method !== "HEAD")
    return { status: 409, data: { message: readOnlyMessage } };
  if (path === "/api/terminals") url.search = "";
  if (path === "/api/terminals/agents") url.search = "";
  if (path === "/api/aow/agents") url.search = "?refresh=true";
  if (path === "/api/aow/automations" && !url.searchParams.has("project_id"))
    url.searchParams.set(
      "project_id",
      snapshot.responses["/api/aow/projects"][0].id,
    );
  if (path.startsWith("/api/fs/raw"))
    url.pathname = path.replace("/api/fs/raw", "/api/fs/text");
  const key = keyFor(url);
  if (Object.hasOwn(snapshot.responses, key)) {
    return {
      status: 200,
      data: structuredClone(snapshot.responses[key]),
      raw: path.startsWith("/api/fs/raw"),
    };
  }
  return { status: 404, data: { message: "这项数据未收录在当前快照中。" } };
}

export function installSnapshotTransport(snapshot) {
  const assetFetch = window.fetch.bind(window);
  window.fetch = async (input, init = {}) => {
    const requestUrl =
      typeof input === "string" || input instanceof URL
        ? String(input)
        : input.url;
    const method =
      init.method || (input instanceof Request ? input.method : "GET");
    let body;
    try {
      body = JSON.parse(init.body || "null");
    } catch {
      /* Reads have no body. */
    }
    const result = snapshotResponse(snapshot, requestUrl, method, body);
    if (!result) return assetFetch(input, init);
    if (init.signal?.aborted) throw new DOMException("Aborted", "AbortError");
    return new Response(
      result.raw ? result.data.content : JSON.stringify(result.data),
      {
        status: result.status,
        headers: {
          "Content-Type": result.raw
            ? result.data.mime || "text/plain"
            : "application/json",
        },
      },
    );
  };

  // Feed the actual xterm/TerminalPaneView component the recorded PTY bytes.
  // Observer mode is the product's existing read-only mode; no command is run.
  class SnapshotSocket extends EventTarget {
    static CONNECTING = 0;
    static OPEN = 1;
    static CLOSING = 2;
    static CLOSED = 3;
    readyState = 0;
    binaryType = "arraybuffer";
    bufferedAmount = 0;
    onopen = null;
    onmessage = null;
    onclose = null;
    onerror = null;
    constructor(url) {
      super();
      this.url = String(url);
      setTimeout(() => {
        if (this.readyState !== 0) return;
        this.readyState = 1;
        this.onopen?.(new Event("open"));
      }, 0);
    }
    message(data) {
      const event = new MessageEvent("message", { data });
      this.onmessage?.(event);
      this.dispatchEvent(event);
    }
    send(input) {
      if (typeof input !== "string" || this.readyState !== 1) return;
      const command = JSON.parse(input);
      if (command.type !== "claim") return;
      if (command.force)
        window.dispatchEvent(
          new CustomEvent("snapshot-notice", { detail: readOnlyMessage }),
        );
      const match = this.url.includes(`/panes/${snapshot.terminal.paneId}/`);
      queueMicrotask(() => {
        if (this.readyState !== 1) return;
        this.message(JSON.stringify({ type: "control", state: "observing" }));
        this.message(
          JSON.stringify({
            type: "stream",
            epoch: snapshot.revision,
            offset: 0,
            reset: true,
            replay_bytes: 0,
            restore_cols: snapshot.terminal.cols,
            restore_rows: snapshot.terminal.rows,
            restore: match ? snapshot.terminal.output : "",
          }),
        );
      });
    }
    close() {
      if (this.readyState === 3) return;
      this.readyState = 3;
      this.onclose?.(new CloseEvent("close", { code: 1000, wasClean: true }));
    }
  }
  window.WebSocket = SnapshotSocket;
  class SnapshotEvents extends EventTarget {
    static OPEN = 1;
    static CLOSED = 2;
    readyState = 1;
    constructor(url) {
      super();
      this.url = String(url);
      setTimeout(() => {
        if (this.readyState === 1 && this.url.endsWith("/operations/stream"))
          this.dispatchEvent(
            new MessageEvent("operations", {
              data: JSON.stringify(
                snapshot.responses["/api/operations/active"],
              ),
            }),
          );
      }, 0);
    }
    close() {
      this.readyState = 2;
    }
  }
  window.EventSource = SnapshotEvents;
  // Uploads use XHR in the real frontend; keep those inside the read-only boundary too.
  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    if (String(url).includes("/api/")) throw new Error(readOnlyMessage);
    return originalOpen.call(this, method, url, ...rest);
  };
}
