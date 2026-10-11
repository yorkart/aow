// Deliberately interleave shared-connection sessions, responses and permissions.
import readline from 'node:readline';
import { existsSync, appendFileSync } from 'node:fs';
const input = readline.createInterface({ input: process.stdin });
const send = message => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`);
let sessions = 0;
const pending = new Map();
input.on('line', line => {
  const { id, method, params } = JSON.parse(line);
  if (!method) { const reply = pending.get(id); pending.delete(id); reply?.(); return; }
  if (method === 'initialize') {
    appendFileSync('initializations.log', 'initialize\n');
    if (existsSync('fail-initialize')) send({ id, error: { code: -32603, message: 'Initialization failed' } });
    else send({ id, result: { protocolVersion: 1, agentCapabilities: { loadSession: true } } });
  } else if (method === 'session/new') {
    if (existsSync('fail-new')) { send({ id, error: { code: -32000, message: 'Authenticate before creation' } }); return; }
    const sessionId = `remote-${++sessions}`;
    setTimeout(() => {
      // Emit before replying to test retrospective attribution.
      send({ method: 'session/update', params: { sessionId, update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: sessionId } } } });
      send({ id, result: { sessionId } });
    }, sessions % 2 ? 25 : 0);
  } else if (method === 'session/prompt') {
    const sessionId = params.sessionId;
    if (params.prompt[0].text === 'url-elicitation') {
      const elicitationId = `url-${sessionId}`;
      pending.set(elicitationId, () => {
        send({ id, result: { stopReason: 'end_turn' } });
        send({ method: 'elicitation/complete', params: { elicitationId } });
      });
      send({ id: elicitationId, method: 'elicitation/create', params: { mode: 'url', sessionId, elicitationId, url: 'https://example.test/login', message: 'Log in' } });
      return;
    }
    const requestId = `permission-${sessionId}`;
    pending.set(requestId, () => {
      send({ method: 'session/update', params: { sessionId, update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: params.prompt[0].text } } } });
      send({ id, result: { stopReason: 'end_turn' } });
    });
    send({ id: requestId, method: 'session/request_permission', params: { sessionId, toolCall: { toolCallId: sessionId, title: sessionId }, options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }] } });
  } else if (method === 'session/load') {
    send({ method: 'session/update', params: { sessionId: params.sessionId, update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Loaded ' + params.sessionId } } } });
    send({ id, result: {} });
  } else send({ id, result: {} });
});
