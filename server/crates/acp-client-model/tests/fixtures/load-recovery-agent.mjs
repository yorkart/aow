import readline from 'node:readline';
import { appendFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';

// Deliberately stricter than the general fixture: load is not idempotent.
const open = new Set();
const loading = new Set();
const write = value => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...value })}\n`);
const reply = (id, result = {}) => write({ id, result });
const fail = (id, message, code = -32603) => write({ id, error: { code, message } });
const event = (method, sessionId, stage = 'start') => appendFileSync('recovery-events.jsonl', `${JSON.stringify({ method, sessionId, stage, pid: process.pid })}\n`);
const update = (sessionId, value) => write({ method: 'session/update', params: { sessionId, update: value } });
const chunk = (sessionId, sessionUpdate, text, messageId) => update(sessionId, { sessionUpdate, messageId, content: { type: 'text', text } });
const gate = async name => {
  while (existsSync(`hold-${name}`) && !existsSync(`release-${name}`)) await new Promise(resolve => setTimeout(resolve, 5));
};

readline.createInterface({ input: process.stdin }).on('line', line => {
  const { id, method, params } = JSON.parse(line);
  void (async () => {
    if (method === 'initialize') {
      if (process.env.PRELOAD_FIRST === '1' && !existsSync('preloaded-once')) { open.add('target'); writeFileSync('preloaded-once', ''); }
      reply(id, { protocolVersion: 1, agentInfo: { name: 'strict-recovery-agent', version: '1' }, agentCapabilities: {
        loadSession: process.env.RESUME_ONLY !== '1', sessionCapabilities: { list: {}, resume: {}, ...(process.env.NO_CLOSE === '1' ? {} : { close: {} }) },
      } });
    } else if (method === 'session/load' || method === 'session/resume') {
      const sid = params.sessionId;
      event(method, sid);
      if (open.has(sid) || loading.has(sid)) { fail(id, 'already loaded'); return; }
      loading.add(sid);
      const answer = existsSync('answer.txt') ? readFileSync('answer.txt', 'utf8') : 'complete answer';
      if (method === 'session/load') chunk(sid, 'user_message_chunk', 'saved question', 'user-1');
      await gate('load');
      loading.delete(sid);
      if (existsSync('fail-load')) {
        const failure = readFileSync('fail-load', 'utf8');
        fail(id, failure, failure === 'cancelled' ? -32800 : failure === 'auth' ? -32000 : -32603);
        return;
      }
      if (method === 'session/load') {
        if (process.env.LONG_HISTORY === '1') {
          for (let index = 0; index < 40; index++) update(sid, { sessionUpdate: 'tool_call', toolCallId: `first-${index}`, title: 'Read', kind: 'read', status: 'completed' });
          chunk(sid, 'agent_message_chunk', 'first conclusion', 'first-final');
          chunk(sid, 'user_message_chunk', 'second question', 'user-2');
          for (let index = 0; index < 149; index++) update(sid, { sessionUpdate: 'tool_call', toolCallId: `second-${index}`, title: 'Read', kind: 'read', status: 'completed' });
        }
        update(sid, { sessionUpdate: 'tool_call', toolCallId: 'tool-1', title: 'Read', kind: 'read', status: 'completed' });
        chunk(sid, 'agent_message_chunk', answer, 'final-1');
      }
      open.add(sid);
      event(method, sid, 'complete');
      reply(id, { configOptions: [] });
    } else if (method === 'session/close') {
      const sid = params.sessionId;
      event(method, sid);
      if (loading.has(sid)) { fail(id, 'cannot close an in-flight load'); return; }
      await gate('close');
      if (existsSync('fail-close')) { fail(id, 'close failed'); return; }
      // This update must stay on the old route, even when a reopen is already waiting.
      if (existsSync('hold-close')) chunk(sid, 'agent_message_chunk', 'old close notification', 'old-close');
      open.delete(sid);
      event(method, sid, 'complete');
      reply(id);
    } else if (method === 'session/prompt') {
      const sid = params.sessionId;
      event(method, sid);
      if (!open.has(sid)) { fail(id, 'session is not loaded'); return; }
      if (params.prompt[0]?.text === 'wait') await gate('prompt');
      chunk(sid, 'agent_message_chunk', 'prompt complete', 'prompt-answer');
      reply(id, { stopReason: 'end_turn' });
    } else if (method === '$/cancel_request' || method === 'session/cancel') {
      // Cooperative cancellation may be ignored by a real agent too.
      event(method, params.sessionId);
    } else if (method === 'session/list') reply(id, { sessions: [] });
    else fail(id, 'Method not found', -32601);
  })().catch(error => { process.stderr.write(String(error)); process.exitCode = 1; });
});
