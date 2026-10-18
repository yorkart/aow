import readline from 'node:readline';
import { appendFileSync, existsSync, readFileSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import path from 'node:path';

const write = message => process.stdout.write(`${JSON.stringify(message)}\n`);
const reply = (id, result) => write({ jsonrpc: '2.0', id, result });
const waiting = new Map();
const log = path.join(process.cwd(), 'lineage-prompts.jsonl');
const update = (sessionId, sessionUpdate, content) => write({ jsonrpc: '2.0', method: 'session/update', params: { sessionId, update: { sessionUpdate, content } } });
readline.createInterface({ input: process.stdin }).on('line', line => {
  const { id, method, params } = JSON.parse(line);
  if (method === 'initialize') {
    reply(id, { protocolVersion: 1, agentInfo: { name: 'lineage-test', version: '1' },
      agentCapabilities: { loadSession: true, sessionCapabilities: { close: {}, delete: {} } }, authMethods: [] });
  } else if (method === 'session/new') {
    setTimeout(() => reply(id, { sessionId: randomUUID() }), Number(process.env.START_DELAY_MS || 0));
  } else if (method === 'session/prompt') {
    appendFileSync(log, `${JSON.stringify({ ...params,
      state_dir: process.env.AOW_STATE_DIR, instance: process.env.AOW_INSTANCE,
      caller_session: process.env.AOW_SESSION_ID, connection: process.env.AOW_ACP_CONNECTION_ID })}\n`);
    if (params.prompt[0]?.text === 'wait') { waiting.set(params.sessionId, id); return; }
    if (params.prompt[0]?.text === 'rename') write({ jsonrpc: '2.0', method: 'session/update', params: {
      sessionId: params.sessionId, update: { sessionUpdate: 'session_info_update', title: 'Provider title' },
    } });
    for (const content of params.prompt) update(params.sessionId, 'user_message_chunk', content);
    update(params.sessionId, 'agent_message_chunk', { type: 'text', text: 'Done' });
    reply(id, { stopReason: 'end_turn' });
  } else if (method === 'session/load') {
    const prompts = existsSync(log) ? readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map(line => JSON.parse(line)) : [];
    for (const prompt of prompts.filter(prompt => prompt.sessionId === params.sessionId)) {
      for (const content of prompt.prompt) update(params.sessionId, 'user_message_chunk', content);
      update(params.sessionId, 'agent_message_chunk', { type: 'text', text: 'Done' });
    }
    reply(id, {});
  } else if (method === 'session/cancel' || method === 'session/close' || method === 'session/delete') {
    const prompt = waiting.get(params.sessionId);
    if (prompt !== undefined) { reply(prompt, { stopReason: 'cancelled' }); waiting.delete(params.sessionId); }
    if (id !== undefined) reply(id, {});
  } else if (id !== undefined) reply(id, {});
});
