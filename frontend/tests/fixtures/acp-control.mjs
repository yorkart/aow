// Shared by multiple browser contexts to model server-owned attachments.
export function acpControlFixture() {
  const sessions = new Map();
  const claims = [];
  let sequence = 0;
  return {
    claims,
    disconnect: id => {
      const session = sessions.get(id);
      if (!session) return;
      const peers = [...session.peers]; session.peers.clear(); session.owner = undefined;
      for (const peer of peers) peer.socket.close();
    },
    authorized: (id, request) => {
      const owner = sessions.get(id)?.owner;
      return !!owner && owner.token === request.headers()['x-aow-acp-control'];
    },
    install: page => page.routeWebSocket('**/api/acp/sessions/*/control*', socket => {
      const id = new URL(socket.url()).pathname.split('/').at(-2);
      let session = sessions.get(id);
      if (!session) { session = { peers: new Set() }; sessions.set(id, session); }
      const peer = { socket, token: '' };
      session.peers.add(peer);
      const broadcast = () => {
        for (const item of session.peers) item.socket.send(JSON.stringify(item === session.owner ? { state: 'claimed', token: item.token } : { state: 'observing' }));
      };
      socket.onMessage(message => {
        const request = JSON.parse(String(message));
        if (request.type === 'claim') {
          claims.push({ id, force: request.force });
          if (!session.owner || request.force && session.owner !== peer) { peer.token = `control-${++sequence}`; session.owner = peer; }
        } else if (request.type === 'observe' && session.owner === peer) session.owner = undefined;
        broadcast();
      });
      socket.onClose(() => {
        session.peers.delete(peer);
        if (session.owner === peer) session.owner = undefined;
        broadcast();
      });
    }),
  };
}
