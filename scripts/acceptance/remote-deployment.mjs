// Run explicitly after building both products. Creates only isolated test resources.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const temporary = mkdtempSync(join(tmpdir(), 'aow-remote-deployment-'));
const prefix = `aow-acceptance-${randomUUID().slice(0, 8)}`;
const network = `${prefix}-network`;
const node = `${prefix}-runtime`;
const server = `${prefix}-server`;
const image = process.env.AOW_ACCEPTANCE_IMAGE || `${prefix}:local`;
const dockerEnv = { ...process.env, BUILDX_CONFIG: join(temporary, 'buildx') };
for (const key of ['DOCKER_HOST', 'DOCKER_CONTEXT', 'DOCKER_TLS', 'DOCKER_TLS_VERIFY', 'DOCKER_CERT_PATH']) delete dockerEnv[key];
function docker(args, input, allowFailure = false) {
  const result = spawnSync('docker', ['--host=unix:///var/run/docker.sock', ...args], { env: dockerEnv, input, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  if (!allowFailure && result.status !== 0) throw new Error(`Docker ${args[0]} failed: ${result.stderr}`);
  return result.stdout.trim();
}
function js(container, source) { return docker(['exec', '-i', container, 'node', '--input-type=module'], source); }
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function ready(container, url) {
  for (let i = 0; i < 60; i++) {
    // Verify readiness by printing a marker, without printing response bodies or credentials.
    const marker = docker(['exec', container, 'node', '-e', `fetch(${JSON.stringify(url)}).then(r=>{if(r.ok)console.log('ready')}).catch(()=>{})`], undefined, true);
    if (marker === 'ready') return;
    await pause(500);
  }
  throw new Error(`Service readiness timed out: ${container}`);
}
const runtimeToken = randomUUID() + randomUUID();
const startup = `import {mkdirSync,writeFileSync} from 'node:fs';import{spawn}from'node:child_process';
writeFileSync('/run/runtime-token',process.env.TEST_RUNTIME_TOKEN,{mode:0o600});
const role=process.argv[1];let args;
if(role==='runtime') {mkdirSync('/node/repository',{recursive:true});args=['aow-runtime-daemon','--state-dir','/node/state','--listen','0.0.0.0:9760','--token-file','/run/runtime-token','--vt-node','/opt/aow/node','--vt-worker','/opt/aow/worker.mjs'];}
else {mkdirSync('/server/state',{recursive:true});const {randomBytes,pbkdf2Sync}=await import('node:crypto');const salt=randomBytes(16);writeFileSync('/server/state/credentials.json',JSON.stringify({version:1,username:'acceptance',salt:salt.toString('hex'),password_hash:pbkdf2Sync('acceptance-password',salt,600000,32,'sha256').toString('hex')}),{mode:0o600});args=['aow-server','--state-dir','/server/state','--host','0.0.0.0','--port','9800','--base-path','/aow','--runtime-url','http://runtime:9760','--runtime-token-file','/run/runtime-token','--server-url','http://server:9800/aow'];}
const child=spawn(args[0],args.slice(1),{stdio:'inherit'});for(const signal of ['SIGTERM','SIGINT'])process.on(signal,()=>child.kill(signal));child.on('exit',code=>process.exit(code??1));`;
try {
  if (!process.env.AOW_ACCEPTANCE_IMAGE) {
    mkdirSync(join(temporary, 'bin'));
    for (const binary of ['aow-server', 'aow-runtime-daemon', 'aow-cli', 'aow-automation-runner']) cpSync(join(root, 'target/debug', binary), join(temporary, 'bin', binary));
    cpSync(process.execPath, join(temporary, 'bin/node'));
    cpSync(join(root, 'runtime/vt-worker/dist/vt-worker.mjs'), join(temporary, 'worker.mjs'));
    cpSync(join(root, 'server/git-platform-providers'), join(temporary, 'git-platform-providers'), { recursive: true });
    writeFileSync(join(temporary, 'Dockerfile'), 'FROM ubuntu:24.04\nRUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates libstdc++6 && rm -rf /var/lib/apt/lists/*\nCOPY bin/ /opt/aow/\nCOPY worker.mjs /opt/aow/worker.mjs\nCOPY git-platform-providers/ /opt/git-platform-providers/\nENV PATH="/opt/aow:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"\n');
    docker(['build', '-t', image, temporary]);
  }
  docker(['network', 'create', '--internal', network]);
  dockerEnv.TEST_RUNTIME_TOKEN = runtimeToken;
  for (const [name, role] of [[node, 'runtime'], [server, 'server']]) {
    docker(['run', '-d', '--init', '--name', name, '--network', network, '--network-alias', role, '-e', 'TEST_RUNTIME_TOKEN', '-e', 'NO_PROXY=server,runtime,localhost,127.0.0.1', image, 'node', '--input-type=module', '-e', startup, role]);
  }
  await ready(server, 'http://server:9800/aow/api/health');
  // The node owns /node. Server has no mount or filesystem path to it.
  assert.equal(js(server, "import{existsSync}from'node:fs';console.log(existsSync('/node'))"), 'false');
  assert.equal(js(node, "import{existsSync}from'node:fs';console.log(existsSync('/server'))"), 'false');
  docker(['exec', node, 'git', '-C', '/node/repository', 'init', '-q']);
  js(node, `import{writeFileSync}from'node:fs';writeFileSync('/opt/aow/callback-agent.sh',${JSON.stringify("#!/bin/sh\nset -eu\ncat >/dev/null\naow-cli --compact project list > /node/callback-result.json\nprintf 'session id: remote-callback-session\\n' >&2\n")},{mode:0o700});`);
  const task = JSON.parse(js(node, `
const base='http://server:9800/aow';
const login=await fetch(base+'/api/auth/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({method:'password',username:'acceptance',password:'acceptance-password'})});
if(!login.ok)throw new Error('Login failed '+login.status);const cookie=login.headers.get('set-cookie').split(';')[0];
async function api(path,method='GET',body){const r=await fetch(base+path,{method,headers:{cookie,'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const text=await r.text();if(!r.ok)throw new Error(path+' '+r.status+' '+text);return JSON.parse(text);}
const project=await api('/api/projects','POST',{path:'/node/repository',notes_path:'/node/notes'});
await api('/api/agents','POST',{id:'callback-codex',agent_type:'codex',display_name:'Callback acceptance',command:'/bin/sh',args:['/opt/aow/callback-agent.sh']});
const task=await api('/api/automations','POST',{project_id:project.id,name:'Remote callback',kind:'manual',prompt:'Verify callback',agent:'codex',agent_id:'callback-codex',workspace_mode:'existing',workspace_path:'/node/repository',base_branch:'',cron:'',max_concurrent_runs:1,enabled:true});
console.log(JSON.stringify({id:task.id,project_id:project.id}));`));
  const runId = randomUUID();
  docker(['exec', server, 'aow-automation-runner', 'run', '--state-dir', '/server/state', '--task-id', task.id, '--run-id', runId, '--source', 'manual']);
  const result = JSON.parse(js(node, "import{readFileSync}from'node:fs';console.log(readFileSync('/node/callback-result.json','utf8'))"));
  assert.ok(result.items.some(item => item.id === task.project_id));
  const journal = js(server, `import{readFileSync}from'node:fs';const rows=readFileSync('/server/state/state/automations/runs/${task.id}/${runId}/events.jsonl','utf8').trim().split(String.fromCharCode(10)).map(JSON.parse);console.log(JSON.stringify(rows.at(-1)));`);
  assert.equal(JSON.parse(journal).status, 'completed');

  const acpFixture=readFileSync(join(root,'scripts/acceptance/fixtures/callback-acp.mjs'),'utf8');
  js(node,`import{writeFileSync}from'node:fs';writeFileSync('/opt/aow/callback-acp.mjs',${JSON.stringify(acpFixture)});`);
  const acpResult=JSON.parse(js(node,`
import{execFileSync}from'node:child_process';import{existsSync,readFileSync}from'node:fs';
const base='http://server:9800/aow';
const login=await fetch(base+'/api/auth/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({username:'acceptance',password:'acceptance-password'})});
if(!login.ok)throw new Error('Login failed');const cookie=login.headers.get('set-cookie').split(';')[0];
const issued=await fetch(base+'/api/cli-tokens',{method:'POST',headers:{cookie,'content-type':'application/json'},body:JSON.stringify({ttl_seconds:300})});if(!issued.ok)throw new Error('Grant failed');const grant=await issued.json();
const headers={authorization:'Bearer '+process.env.TEST_RUNTIME_TOKEN,'content-type':'application/json'};
const settings=await (await fetch('http://runtime:9760/v1/acp/settings',{headers})).json();
const changed=await fetch('http://runtime:9760/v1/acp/settings',{method:'PUT',headers,body:JSON.stringify({revision:settings.revision,content:JSON.stringify({default_agent_id:'callback-acp',agent_servers:{'callback-acp':{type:'custom',command:'/opt/aow/node',args:['/opt/aow/callback-acp.mjs']}}})})});if(!changed.ok)throw new Error('Node ACP settings rejected');
execFileSync('aow-cli',['acp','create','--agent-id','callback-acp','--project-id',${JSON.stringify(task.project_id)},'--cwd','/node/repository','--no-parent','--session-id','acceptance-controller','--task','Verify ACP callback','--timeout','20'],{env:{...process.env,AOW_SERVER_URL:base,AOW_SERVER_TOKEN:grant.token},encoding:'utf8'});
for(let i=0;i<100 && !existsSync('/node/acp-callback-result.json');i++)await new Promise(resolve=>setTimeout(resolve,100));
console.log(readFileSync('/node/acp-callback-result.json','utf8'));
`));
  assert.ok(acpResult.items.some(item=>item.id===task.project_id));
  console.log(JSON.stringify({ result: 'passed', isolated_filesystems: true, transport: 'HTTP over isolated Docker network', callback: 'CLI automation and native ACP agents -> business CLI -> Server -> node execution', shared_state_mounts: false }));
} finally {
  for (const name of [server, node]) docker(['rm', '-f', name], undefined, true);
  docker(['network', 'rm', network], undefined, true);
  if (!process.env.AOW_ACCEPTANCE_IMAGE) docker(['image', 'rm', image], undefined, true);
  rmSync(temporary, { recursive: true, force: true });
}
