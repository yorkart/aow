// Transport fixture for UI tests; the production transport is exercised by base-path.test.mjs.
export async function installLiveEvents(context, workspace = { boot_id: 'watch-fixture', revision: 0, projects: 0, terminals: 0, repositories: {} }, terminals) {
  if (terminals) await context.exposeBinding('__terminalSnapshot', () => terminals());
  await context.addInitScript(({ workspace, terminals }) => {
    const NativeSocket = window.WebSocket;
    window.liveEventSockets = [];
    window.workspaceSnapshot = workspace;
    window.liveEventsOffline = false;
    let terminalRevision = 0;
    let terminalKey;
    class LiveSocket {
      constructor(url) {
        this.url = String(url); this.readyState = 0;
        window.liveEventSockets.push(this);
        queueMicrotask(() => {
          if (this.readyState === 3) return;
          if (window.liveEventsOffline) { this.close(); return; }
          this.readyState = 1; this.onopen?.(new Event('open'));
          this.receive('workspace', window.workspaceSnapshot);
          if (terminals) this.receive('terminals', { type: 'hello', protocol: 1 });
        });
      }
      send(raw) {
        const command = JSON.parse(raw);
        if (command.action === 'subscribe_terminals') {
          this.terminalActive = true;
          void this.sampleTerminals(true);
          clearInterval(this.terminalTimer);
          this.terminalTimer = setInterval(() => { void this.sampleTerminals(); }, 1500);
        } else if (command.action === 'unsubscribe_terminals') {
          this.terminalActive = false; clearInterval(this.terminalTimer);
        } else if (command.action === 'refresh_terminals') {
          void this.sampleTerminals(true, command.id);
        }
      }
      async sampleTerminals(force = false, requestId) {
        if (!this.terminalActive || this.readyState !== 1) return;
        if (this.terminalPending) {
          if (force) { await this.terminalPending; return this.sampleTerminals(true, requestId); }
          return;
        }
        this.terminalPending = window.__terminalSnapshot();
        try {
          const snapshot = await this.terminalPending;
          if (!this.terminalActive || this.readyState !== 1) return;
          const key = JSON.stringify(snapshot);
          const changed = key !== terminalKey;
          if (changed) { terminalKey = key; terminalRevision++; }
          if (changed || force) this.receive('terminals', { type: 'snapshot', request_id: requestId,
            state: { boot_id: 'terminal-fixture', revision: terminalRevision, ...snapshot } });
        } finally { this.terminalPending = undefined; }
      }
      receive(event, data) { this.onmessage?.(new MessageEvent('message', { data: JSON.stringify({ event, data }) })); }
      close() {
        if (this.readyState === 3) return;
        this.readyState = 3;
        this.terminalActive = false; clearInterval(this.terminalTimer);
        this.onclose?.({ code: 1006 });
      }
    }
    window.WebSocket = new Proxy(NativeSocket, { construct(Target, args) {
      return String(args[0]).includes('/api/events/ws') ? new LiveSocket(args[0]) : Reflect.construct(Target, args);
    } });
    window.emitLiveEvent = (event, data) => {
      for (const socket of window.liveEventSockets) if (socket.readyState === 1) socket.receive(event, data);
    };
  }, { workspace, terminals: !!terminals });
}
