import readline from 'node:readline';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

// Deliberately local fake runtime: no provider calls or credentials are required.
const mode = process.env.PROBE_MODE;
const output = prompt => {
  assert.equal(prompt, 'ping', 'connection probes must send only the fixed ping prompt');
  if (mode === 'empty') return '';
  if (mode === 'whitespace') return ' \t\n';
  const identityHome = process.env[process.env.PROBE_REQUIRED_HOME ?? 'HOME'];
  if (process.env.PROBE_REQUIRED_FILE && readFileSync(path.join(identityHome, process.env.PROBE_REQUIRED_FILE), 'utf8') !== 'opaque identity bytes') {
    throw new Error('Identity file was not imported into the runtime Home');
  }
  writeFileSync(path.join(process.env.HOME, 'tested'), 'refreshed');
  return process.env.PROBE_REPLY ?? 'pong';
};
if (mode === 'timeout') {
  writeFileSync(process.env.PROBE_PID, String(process.pid));
  setTimeout(() => writeFileSync(process.env.PROBE_LATE, 'still running'), 1500);
} else if (mode === 'fail') {
  process.stdout.write('pong\n');
  process.stderr.write('private provider error');
  process.exitCode = 1;
} else if (process.argv.includes('--acp')) {
  const write = message => process.stdout.write(`${JSON.stringify(message)}\n`);
  readline.createInterface({ input: process.stdin }).on('line', line => {
    const { id, method, params } = JSON.parse(line);
    if (id === undefined) return;
    let result = {};
    if (method === 'initialize') result = { protocolVersion: 1, agentCapabilities: {}, authMethods: [] };
    if (method === 'session/new') {
      if (mode === 'auth') { write({ jsonrpc: '2.0', id, error: { code: -32000, message: 'Login required' } }); return; }
      result = { sessionId: 'probe-session' };
    }
    if (method === 'session/prompt') {
      assert.equal(params.prompt.length, 1, 'connection probes must not inject role instructions');
      write({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: params.sessionId, update: {
        sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: output(params.prompt[0].text) },
      } } });
      if (mode === 'prompt-error') { write({ jsonrpc: '2.0', id, error: { code: -32603, message: 'Request failed after replying' } }); return; }
      result = { stopReason: 'end_turn' };
    }
    write({ jsonrpc: '2.0', id, result });
  });
} else {
  const prompt = process.argv.find(arg => arg === 'ping') ?? readFileSync(0, 'utf8');
  process.stdout.write(`${output(prompt)}\n`);
}
