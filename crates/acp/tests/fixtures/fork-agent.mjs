import readline from 'node:readline';
import { appendFileSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const file = path.join(process.cwd(), 'fork-sessions.json');
const sessions = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : {};
const write = value => process.stdout.write(`${JSON.stringify({ jsonrpc: '2.0', ...value })}\n`);
const reply = (id, result) => write({ id, result });
const save = () => writeFileSync(file, JSON.stringify(sessions));
const update = (sessionId, sessionUpdate, text) => write({ method: 'session/update', params: { sessionId, update: { sessionUpdate, content: { type: 'text', text } } } });
const configuration = { modes: { currentModeId: 'ask', availableModes: [{ id: 'ask', name: 'Ask' }, { id: 'code', name: 'Code' }] }, configOptions: [{ id: 'model', name: 'Model', type: 'select', currentValue: 'inherited', options: [{ value: 'inherited', name: 'Inherited' }, { value: 'default', name: 'Default' }] }] };

readline.createInterface({ input: process.stdin }).on('line', async line => {
  const { id, method, params } = JSON.parse(line);
  if (!method) return;
  appendFileSync(path.join(process.cwd(), 'fork-requests.jsonl'), `${JSON.stringify({ method, params })}\n`);
  if (method === 'initialize') {
    reply(id, { protocolVersion: 1, agentCapabilities: { loadSession: process.env.NO_LOAD !== '1', sessionCapabilities: { fork: JSON.parse(process.env.FORK_CAPABILITY ?? '{}'), resume: {}, close: {}, delete: {} } } });
  } else if (method === 'session/new') {
    const sessionId = `native-${Object.keys(sessions).length}`;
    sessions[sessionId] = []; save(); reply(id, { sessionId, ...configuration });
  } else if (method === 'session/fork') {
    while (existsSync(path.join(process.cwd(), 'hold-fork'))) await new Promise(resolve => setTimeout(resolve, 5));
    if (process.env.FORK_DELAY) await new Promise(resolve => setTimeout(resolve, Number(process.env.FORK_DELAY)));
    if (process.env.FORK_ERROR === '1') { write({ id, error: { code: -32603, message: 'Fork rejected' } }); return; }
    const sessionId = process.env.FORK_SAME_ID === '1' ? params.sessionId : `native-${Object.keys(sessions).length}`;
    sessions[sessionId] = structuredClone(sessions[params.sessionId]);
    if (process.env.FORK_REWRITE_HISTORY === '1') {
      sessions[sessionId].find(entry => entry.role === 'assistant').text = 'Different native result';
    }
    save();
    // Like codex-acp, forking returns configuration without replaying any messages.
    reply(id, { sessionId, ...configuration });
  } else if (method === 'session/load') {
    if (existsSync(path.join(process.cwd(), 'fail-fork-load'))) { write({ id, error: { code: -32603, message: 'Fork replay failed' } }); return; }
    for (const entry of sessions[params.sessionId]) update(params.sessionId, entry.role === 'user' ? 'user_message_chunk' : 'agent_message_chunk', entry.text);
    reply(id, configuration);
  } else if (method === 'session/resume') reply(id, configuration);
  else if (method === 'session/prompt') {
    if (params.prompt[0].text === 'hold') return;
    const history = sessions[params.sessionId];
    const text = params.prompt[0].text;
    const response = `History: ${history.filter(entry => entry.role === 'user').map(entry => entry.text).join(', ')}; reply: ${text}`;
    history.push({ role: 'user', text }, { role: 'assistant', text: response }); save();
    update(params.sessionId, 'agent_message_chunk', response);
    if (process.env.PROMPT_ERROR === '1') write({ id, error: { code: -32603, message: 'Prompt failed after partial output' } });
    else reply(id, { stopReason: process.env.PROMPT_STOP_REASON || 'end_turn' });
  } else if (method === 'session/set_mode' || method === 'session/close' || method === 'session/delete') reply(id, {});
  else if (method === 'session/set_config_option') reply(id, { configOptions: configuration.configOptions });
  else write({ id, error: { code: -32601, message: 'Method not found' } });
});
