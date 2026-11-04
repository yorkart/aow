// Minimal native ACP adapter. Its callback must receive Server credentials through the node wire.
import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { createInterface } from 'node:readline';
const sessionId = 'acceptance-acp-session';
const send = message => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`);
for await (const line of createInterface({ input: process.stdin })) {
  const message = JSON.parse(line);
  switch (message.method) {
    case 'initialize':
      send({ id: message.id, result: { protocolVersion: 1, agentCapabilities: { loadSession: true }, authMethods: [] } });
      break;
    case 'session/new':
    case 'session/load':
      send({ id: message.id, result: { sessionId } });
      break;
    case 'session/prompt': {
      const reply = execFileSync('aow-cli', ['--compact', 'project', 'list'], { encoding: 'utf8' });
      writeFileSync('/node/acp-callback-result.json', reply);
      send({ method: 'session/update', params: { sessionId, update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'ACP callback accepted' } } } });
      send({ id: message.id, result: { stopReason: 'end_turn' } });
      break;
    }
    default:
      if ('id' in message) send({ id: message.id, result: {} });
  }
}
