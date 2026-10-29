import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
if (args.includes('--version')) { console.log('fixture-cli 1.0.0'); process.exit(0); }
if (args.includes('--device-auth')) {
  console.log('Visit https://auth.openai.com/codex/device and enter this code: ABCD-EFGH');
  if (process.env.ORBIT_FIXTURE_DEVICE_MODE === 'wait') setInterval(() => {}, 1000);
  else setTimeout(() => {
    if (process.env.ORBIT_FIXTURE_DEVICE_MODE === 'failure') process.exit(1);
    writeFileSync(join(process.env.CODEX_HOME, 'auth.json'), JSON.stringify({ auth_mode: 'apikey', OPENAI_API_KEY: 'device-flow-fixture-identity' }));
    process.exit(0);
  }, Number(process.env.ORBIT_FIXTURE_DEVICE_DELAY ?? 150));
} else if (args.includes('setup-token')) {
  if (!process.stdin.isTTY) process.exit(3);
  console.log('Open the following URL to continue:');
  console.log('https://claude.ai/oauth/authorize?response_type=code&client_id=fixture&state=fixture-state&code_challenge=fixture-challenge&code_challenge_method=S256&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback');
  console.log('Paste code here:');
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', value => {
    if (value.trim() !== 'fixture-code#state') process.exit(1);
    console.log('Your OAuth token (valid for 1 year):');
    console.log('sk-ant-oat01-fixture-credential-value-with-a-long-safe-tail');
    console.log("Store this token securely. You won't be able to see it again.");
    process.exit(0);
  });
} else if (args.includes('login') && args.includes('status')) {
  try { const value = JSON.parse(readFileSync(join(process.env.CODEX_HOME, 'auth.json'), 'utf8')); if (!value.OPENAI_API_KEY && !value.tokens?.access_token) process.exit(1); console.log('Logged in'); }
  catch { process.exit(1); }
} else if (args.includes('auth') && args.includes('status')) {
  let loggedIn = Boolean(process.env.CLAUDE_CODE_OAUTH_TOKEN || process.env.ANTHROPIC_API_KEY || process.env.ANTHROPIC_AUTH_TOKEN);
  try { loggedIn ||= Boolean(JSON.parse(readFileSync(join(process.env.CLAUDE_CONFIG_DIR, '.credentials.json'), 'utf8')).claudeAiOauth?.accessToken); } catch { /* No native auth file. */ }
  console.log(JSON.stringify({ loggedIn, authMethod: 'claude.ai' }));
  process.exit(loggedIn ? 0 : 1);
} else process.exit(2);
