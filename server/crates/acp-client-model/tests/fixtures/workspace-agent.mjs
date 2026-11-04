import { appendFileSync } from 'node:fs';
import readline from 'node:readline';
import { randomUUID } from 'node:crypto';

const reply = (id, result) => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', id, result })}\n`);
readline.createInterface({ input: process.stdin }).on('line', line => {
  const { id, method, params } = JSON.parse(line);
  appendFileSync(process.env.ACP_WORKSPACE_RPC_LOG, `${method}\n`);
  if (method === 'initialize') {
    reply(id, { protocolVersion: 1, agentInfo: { name: 'workspace-test', version: '1' },
      agentCapabilities: { sessionCapabilities: { close: {}, delete: {} } }, authMethods: [] });
  } else if (method === 'session/new') {
    reply(id, { sessionId: randomUUID() });
  } else if (method === 'session/prompt' && params.prompt[0]?.text === 'hello') {
    setTimeout(() => {
      process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: params.sessionId, update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Workspace reply' } } } })}\n`);
      reply(id, { stopReason: 'end_turn' });
    }, 200);
  } else if (method !== 'session/prompt' && id !== undefined) {
    reply(id, {});
  }
});
