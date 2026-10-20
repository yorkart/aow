import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import react from '@vitejs/plugin-react';
import { WebSocketServer } from 'ws';

// Deterministic API data for both the real app shell and the integration preview.
// All requests stay in this fixture; no user repositories or services are changed.
export async function themePreviewServer(port = 0) {
  const state = { connections: 0, claims: 0, inputs: [] };
  const server = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../../', import.meta.url)),
    server: { host: '127.0.0.1', port, strictPort: true, proxy: {} },
    plugins: [react(), {
      name: 'theme-preview-api',
      configureServer(server) {
        server.middlewares.use('/api', (request, response) => {
          const path = new URL(request.url, 'http://localhost').pathname;
          response.setHeader('Content-Type', 'application/json');
          const data = {
            '/auth/status': { configured: true, authenticated: true },
            '/aow/projects': [],
            '/aow/settings': { notes_base: '/preview/notes', execution_path: ['/usr/bin'], node_addresses: [], editor: { word_wrap: false } },
            '/aow/agents': [], '/aow/automations': [], '/aow/agent-sessions': [],
            '/aow/pinned-worktrees': { paths: [], revision: 0 },
            '/aow/notification-settings': { im: { providers: [] }, notifications: { agent_task_completed: { enabled: false, channels: [] } } },
            '/terminals': { tabs: [] }, '/terminals/agents': { agents: {}, titles: {}, processes: {} },
            '/aow/settings/configuration': { selection: { config_repo: '', config_id: '' }, active_selection: { config_repo: '', config_id: '' }, restart_required: false },
            '/aow/review-providers': { providers: [] }, '/aow/acp/agents': [],
            '/operations/active': [],
          };
          if (path === '/terminals/task-stops') {
            response.setHeader('Content-Type', 'text/event-stream'); response.end(': ready\n\n'); return;
          }
          if (request.method !== 'GET' || !(path in data)) {
            response.statusCode = 404;
            response.end(JSON.stringify({ message: `No theme fixture for ${request.method} ${path}` }));
            return;
          }
          response.end(JSON.stringify(data[path]));
        });
      },
    }],
  });
  const websocket = new WebSocketServer({ noServer: true });
  server.httpServer.on('upgrade', (request, socket, head) => {
    if (!request.url.startsWith('/api/')) return;
    websocket.handleUpgrade(request, socket, head, ws => {
      if (!request.url.includes('/terminals/')) return;
      state.connections++;
      ws.on('message', (data, binary) => {
        if (binary) { state.inputs.push(data.toString()); ws.send(data); return; }
        const control = JSON.parse(data.toString());
        if (control.type !== 'claim') return;
        state.claims++;
        ws.send(JSON.stringify({ type: 'control', state: 'claimed' }));
        ws.send(JSON.stringify({
          type: 'stream', epoch: 'theme-preview', offset: 0, reset: true, replay_bytes: 0,
          restore_cols: 80, restore_rows: 24,
          restore: '\x1b[2J\x1b[HTheme preview — terminal stays connected\r\n\x1b[31mred \x1b[32mgreen \x1b[33myellow \x1b[34mblue \x1b[35mmagenta \x1b[36mcyan\x1b[0m\r\n$ ',
        }));
      });
    });
  });
  await server.listen();
  return {
    url: `http://127.0.0.1:${server.httpServer.address().port}`,
    state,
    close: async () => { for (const client of websocket.clients) client.terminate(); websocket.close(); await server.close(); },
  };
}
