import readline from 'node:readline';
import { appendFileSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import path from 'node:path';

const write = message => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`);
const reply = (id, result) => write({ id, result });
const sessions = new Map();
const update = (sessionId, value) => write({ method: 'session/update', params: { sessionId, update: value } });
const activity = (sessionId, type) => update(sessionId, { sessionUpdate: 'session_info_update', _meta: { codex: { threadStatus: { type } } } });
const message = (sessionId, role, text, messageId) => update(sessionId, { sessionUpdate: `${role}_message_chunk`, messageId, content: { type: 'text', text } });
function finish(sessionId, reason = 'end_turn') {
  const session = sessions.get(sessionId);
  if (session.prompt !== undefined) { reply(session.prompt, { stopReason: reason }); delete session.prompt; }
  activity(sessionId, 'idle');
}
readline.createInterface({ input: process.stdin }).on('line', line => {
  const request = JSON.parse(line);
  const { id, method, params } = request;
  appendFileSync(path.join(process.cwd(), 'steering-wire.jsonl'), `${line}\n`);
  if (method === 'initialize') {
    reply(id, { protocolVersion: 1, agentInfo: { name: 'generic-agent', version: '1' }, agentCapabilities: { loadSession: true }, authMethods: [],
      _meta: { steering: { supported: process.env.STEERING !== '0' } } });
  } else if (method === 'session/new') {
    const sessionId = randomUUID(); sessions.set(sessionId, {}); reply(id, { sessionId });
  } else if (method === 'session/prompt') {
    const session = sessions.get(params.sessionId);
    session.prompt = id;
    session.task = params.prompt[0].text;
    activity(params.sessionId, 'active');
    for (const content of params.prompt) update(params.sessionId, { sessionUpdate: 'user_message_chunk', content });
    if (session.task === 'early-idle') activity(params.sessionId, 'idle');
    message(params.sessionId, 'agent', 'Before steering', 'before');
    if (session.task === 'replay') {
      update(params.sessionId, { sessionUpdate: 'tool_call', toolCallId: 'inspect', title: 'Inspect', status: 'in_progress' });
      update(params.sessionId, { sessionUpdate: 'tool_call_update', toolCallId: 'inspect', status: 'completed' });
      message(params.sessionId, 'agent', 'Answer ', 'answer');
    }
    if (params.prompt[0].text === 'prompt-required') finish(params.sessionId);
    if (params.prompt[0].text === 'permission') write({ id: `permission-${params.sessionId}`, method: 'session/request_permission', params: {
      sessionId: params.sessionId, toolCall: { toolCallId: 'tool', title: 'Approval remains pending', status: 'pending' },
      options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }],
    } });
  } else if (method === '_session/steering') {
    const session = sessions.get(params.sessionId);
    const text = params.prompt[0].text;
    if (text === 'hold') { session.steer = id; return; }
    if (text === 'prompt-required') {
      finish(params.sessionId);
      reply(id, { outcome: 'promptRequired', reason: 'noRunningTurn' }); return;
    }
    if (text === 'failed' || text === 'unexpected') { reply(id, { outcome: text }); return; }
    for (const content of params.prompt) {
      if (content.type === 'text') {
        message(params.sessionId, 'user', content.text.slice(0, 3));
        message(params.sessionId, 'user', content.text.slice(3));
      } else update(params.sessionId, { sessionUpdate: 'user_message_chunk', content });
    }
    if (session.task === 'replay') {
      message(params.sessionId, 'agent', 'complete', 'answer');
      message(params.sessionId, 'agent', 'Followup response', 'followup');
      reply(id, { outcome: 'injected' }); finish(params.sessionId); return;
    }
    if (text.includes('new-turn')) {
      if (session.task === 'early-idle') { reply(session.prompt, { stopReason: 'end_turn' }); delete session.prompt; }
      else finish(params.sessionId);
      if (text === 'late-new-turn') {
        reply(id, { outcome: 'startedNewTurn' });
        setTimeout(() => { activity(params.sessionId, 'active'); message(params.sessionId, 'agent', 'Detached response'); activity(params.sessionId, 'idle'); }, 30);
        return;
      }
      activity(params.sessionId, 'active');
      message(params.sessionId, 'agent', 'Detached response');
      if (text === 'fast-new-turn') activity(params.sessionId, 'idle');
      reply(id, { outcome: 'startedNewTurn' }); return;
    }
    message(params.sessionId, 'agent', 'After steering');
    if (text === 'finish-before-ack') finish(params.sessionId);
    reply(id, { outcome: 'injected' });
  } else if (method === 'session/load') {
    message(params.sessionId, 'user', 'replay');
    message(params.sessionId, 'agent', 'Before steering', 'before');
    update(params.sessionId, { sessionUpdate: 'tool_call', toolCallId: 'inspect', title: 'Inspect', status: 'completed' });
    message(params.sessionId, 'agent', 'Answer complete', 'answer');
    message(params.sessionId, 'user', 'Correction');
    message(params.sessionId, 'agent', 'Followup response', 'followup');
    reply(id, {});
  } else if (method === 'session/cancel') {
    const session = sessions.get(params.sessionId);
    finish(params.sessionId, 'cancelled');
    if (session.steer !== undefined) { reply(session.steer, { outcome: 'promptRequired', reason: 'noRunningTurn' }); delete session.steer; }
  } else if (method && id !== undefined) reply(id, {});
});
