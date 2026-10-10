import readline from 'node:readline';
import { appendFileSync, existsSync } from 'node:fs';
import path from 'node:path';
const input = readline.createInterface({ input: process.stdin });
const write = value => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...value })}\n`);
const reply = (id, result) => write({ id, result });
const update = value => write({ method: 'session/update', params: { sessionId: 'session-1', update: value } });
const chunk = (sessionUpdate, text) => update({ sessionUpdate, content: { type: 'text', text } });
let serial = 0;
let prompt;
let options = [
  { id: 'enabled', name: 'Enabled', type: 'boolean', currentValue: false },
  { id: 'model', name: 'Model', type: 'select', currentValue: 'small', options: [{ value: 'small', name: 'Small' }] },
];
const pending = new Map();
const request = (method, params, callback) => { const id = `request-${++serial}`; pending.set(id, callback); write({ id, method, params }); };
const finish = id => reply(id, { stopReason: 'end_turn' });
input.on('line', line => {
  const message = JSON.parse(line);
  if (!message.method) { const callback = pending.get(message.id); pending.delete(message.id); callback?.(message); return; }
  const { id, method, params } = message;
  if (method === 'initialize') {
    if (params.clientCapabilities.terminal) throw new Error('Must not advertise terminal capability');
    if (!params.clientCapabilities._meta?.terminal_output) throw new Error('Must advertise display-only terminal output');
    reply(id, { protocolVersion: 1, agentInfo: { name: 'test-agent', version: process.env.MARKER || '1' }, agentCapabilities: { loadSession: process.env.NO_LOAD !== '1', sessionCapabilities: { list: {}, close: {}, delete: {}, ...(process.env.RESUME_ONLY === '1' ? { resume: {} } : {}) } }, authMethods: [{ id: 'login', name: 'Login' }] });
  } else if (method === 'session/new') {
    reply(id, { sessionId: 'session-1', modes: { currentModeId: 'ask', availableModes: [{ id: 'ask', name: 'Ask' }, { id: 'code', name: 'Code' }] }, ...(process.env.LEGACY_MODES === '1' ? {} : { configOptions: process.env.EMPTY_CONFIG === '1' ? [] : process.env.NULL_CONFIG === '1' ? null : options }) });
  } else if (method === 'session/load') {
    if (existsSync(path.join(process.cwd(), 'fail-load'))) { chunk('agent_message_chunk', 'incomplete replay'); write({ id, error: { code: -32603, message: 'load failed' } }); return; }
    chunk('user_message_chunk', 'previous prompt');
    chunk('agent_message_chunk', 'previous answer');
    reply(id, { configOptions: options });
  } else if (method === 'session/resume') {
    reply(id, { configOptions: options });
  } else if (method === 'session/list') {
    reply(id, { sessions: [{ sessionId: 'session-1', cwd: process.cwd(), title: 'Saved conversation' }] });
  } else if (method === 'session/set_mode') {
    reply(id, {}); update({ sessionUpdate: 'current_mode_update', currentModeId: params.modeId });
  } else if (method === 'session/set_config_option') {
    options = options.map(option => option.id === params.configId ? { ...option, currentValue: params.value } : option);
    if (params.configId === 'enabled' && params.value) options[1] = { ...options[1], options: [{ value: 'small', name: 'Small' }, { value: 'large', name: 'Large' }] };
    reply(id, { configOptions: options });
  } else if (method === 'authenticate') {
    request('elicitation/create', { mode: 'form', requestId: id, message: 'Login code', requestedSchema: { type: 'object', properties: { code: { type: 'string' } }, required: ['code'] } }, response => { if (response.result?.content?.code !== '1234') throw new Error('Incorrect login response'); reply(id, {}); });
  } else if (method === 'session/prompt') {
    appendFileSync(path.join(process.cwd(), 'prompts.log'), `${params.prompt[0].text}\n`);
    const text = params.prompt[0].text;
    if (text === 'auth-required') { write({ id, error: { code: -32000, message: '请重新登录' } }); return; }
    if (text === 'checkpoint') { chunk('agent_message_chunk', 'Unfinished streamed response'); prompt = id; return; }
    if (text === 'cancel') { prompt = id; return; }
    if (text === 'terminal') {
      update({ sessionUpdate: 'tool_call', toolCallId: 'command', title: 'pwd', kind: 'execute', status: 'in_progress', content: [{ type: 'terminal', terminalId: 'terminal-1' }], _meta: { terminal_info: { terminal_id: 'terminal-1', cwd: process.cwd() } } });
      for (const data of ['first line\n', 'second line\n']) update({ sessionUpdate: 'tool_call_update', toolCallId: 'command', _meta: { terminal_output: { terminal_id: 'terminal-1', data } } });
      update({ sessionUpdate: 'tool_call_update', toolCallId: 'command', status: 'completed', _meta: { terminal_exit: { terminal_id: 'terminal-1', exit_code: 0 } } });
      chunk('agent_message_chunk', 'Command completed'); finish(id); return;
    }
    if (text === 'exit') { chunk('agent_message_chunk', 'last output'); process.exitCode = 7; input.close(); process.stdin.destroy(); return; }
    if (text === 'permission') {
      request('session/request_permission', { sessionId: 'session-1', toolCall: { toolCallId: 'tool-1', title: 'Write file', kind: 'edit', status: 'pending', content: [{ type: 'diff', path: path.join(process.cwd(), 'source.txt'), oldText: 'before', newText: 'after' }] }, options: [{ optionId: 'allow', name: 'Allow once', kind: 'allow_once' }, { optionId: 'reject', name: 'Reject', kind: 'reject_once' }] }, response => { chunk('agent_message_chunk', JSON.stringify(response.result)); finish(id); }); return;
    }
    if (text === 'activity') {
      chunk('agent_message_chunk', '**Hel');
      update({ sessionUpdate: 'plan', entries: [{ content: 'Read file', priority: 'high', status: 'completed' }] });
      update({ sessionUpdate: 'notice', severity: 'warning', title: 'Rate limit', description: 'Please **wait** before retrying.' });
      chunk('agent_message_chunk', 'lo**');
      update({ sessionUpdate: 'compaction_update', compactionId: 'compact-1', status: 'in_progress' });
      for (const text of ['**Sum', 'mary**']) update({ sessionUpdate: 'compaction_summary_chunk', compactionId: 'compact-1', content: { type: 'text', text } });
      update({ sessionUpdate: 'compaction_update', compactionId: 'compact-1', status: 'completed' });
      finish(id); return;
    }
    if (text === 'files') {
      request('fs/read_text_file', { sessionId: 'session-1', path: path.join(process.cwd(), 'source.txt'), line: 2, limit: 1 }, response => {
        request('fs/write_text_file', { sessionId: 'session-1', path: path.join(process.cwd(), 'written.txt'), content: response.result.content }, () => finish(id));
      }); return;
    }
    chunk('user_message_chunk', text.slice(0, 2)); chunk('user_message_chunk', text.slice(2));
    chunk('agent_thought_chunk', 'Thinking'); chunk('agent_message_chunk', 'Hello'); chunk('agent_message_chunk', ' world');
    update({ sessionUpdate: 'tool_call', toolCallId: 'tool-1', title: 'Read file', status: 'in_progress', kind: 'read', locations: [{ path: path.join(process.cwd(), 'source.txt'), line: 2 }] });
    update({ sessionUpdate: 'tool_call_update', toolCallId: 'tool-1', status: 'completed', content: [{ type: 'content', content: { type: 'text', text: 'tool output' } }] });
    update({ sessionUpdate: 'plan', entries: [{ content: 'Read file', priority: 'high', status: 'completed' }] });
    update({ sessionUpdate: '_future', privateData: { preserve: true } });
    finish(id);
  } else if (method === 'session/cancel') {
    if (prompt != null) { reply(prompt, { stopReason: 'cancelled' }); prompt = undefined; }
  } else if (method === 'session/close' || method === 'session/delete') reply(id, {});
  else write({ id, error: { code: -32601, message: 'Method not found' } });
});
