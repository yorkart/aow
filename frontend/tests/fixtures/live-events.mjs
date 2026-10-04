// Transport fixture for UI tests; the production transport is exercised by base-path.test.mjs.
export async function installLiveEvents(context, workspace = { boot_id: 'watch-fixture', revision: 0, projects: 0, terminals: 0, repositories: {} }) {
  await context.addInitScript(workspace => {
    const NativeSocket = window.WebSocket;
    window.liveEventSockets = [];
    window.workspaceSnapshot = workspace;
    window.liveEventsOffline = false;
    class LiveSocket {
      constructor(url) {
        this.url = String(url); this.readyState = 0;
        window.liveEventSockets.push(this);
        queueMicrotask(() => {
          if (this.readyState === 3) return;
          if (window.liveEventsOffline) { this.close(); return; }
          this.readyState = 1; this.onopen?.(new Event('open'));
          this.receive('workspace', window.workspaceSnapshot);
        });
      }
      receive(event, data) { this.onmessage?.(new MessageEvent('message', { data: JSON.stringify({ event, data }) })); }
      close() {
        if (this.readyState === 3) return;
        this.readyState = 3;
        this.onclose?.({ code: 1006 });
      }
    }
    window.WebSocket = new Proxy(NativeSocket, { construct(Target, args) {
      return String(args[0]).includes('/api/events/ws') ? new LiveSocket(args[0]) : Reflect.construct(Target, args);
    } });
    window.emitLiveEvent = (event, data) => {
      for (const socket of window.liveEventSockets) if (socket.readyState === 1) socket.receive(event, data);
    };
  }, workspace);
}
