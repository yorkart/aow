// A stdio ACP adapter exercising responses, notifications and client-bound requests.
import { createInterface } from 'node:readline';
import { spawn } from 'node:child_process';

const descendant = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
const send = message => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`);
let prompt;
for await (const line of createInterface({ input: process.stdin })) {
  const message = JSON.parse(line);
  switch (message.method) {
    case 'initialize':
      send({ id: message.id, result: { protocolVersion: 1, agentCapabilities: { loadSession: true }, authMethods: [],
        _meta: { pid: process.pid, descendant: descendant.pid, state_dir: process.env.AOW_STATE_DIR,
          home: process.env.HOME, sentinel: process.env.AOW_TEST_NODE_VALUE } } });
      break;
    case 'session/new':
      send({ id: message.id, result: { sessionId: 'native-session', _meta: { preserved: true } } });
      break;
    case 'session/prompt':
      prompt = message.id;
      send({ id: 'permission-1', method: 'session/request_permission', params: { sessionId: 'native-session',
        toolCall: { toolCallId: 'tool-1', title: 'Read file', kind: 'read', status: 'pending' },
        options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }] } });
      break;
    case 'session/cancel':
      send({ id: prompt, result: { stopReason: 'cancelled' } });
      break;
    default:
      if (message.id === 'permission-1') {
        send({ method: 'session/update', params: { sessionId: 'native-session', update: {
          sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'permission received' } } } });
        send({ id: prompt, result: { stopReason: 'end_turn' } });
      } else {
        send({ id: message.id, result: message.params ?? {} });
      }
  }
}
