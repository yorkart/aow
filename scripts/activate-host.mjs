import { spawn } from 'node:child_process';

// A new session lets activation
// finish even when stopping Host daemon closes the terminal running the installer.
// Normally the caller still waits and receives the actual activation result.
const child = spawn('/bin/sh', process.argv.slice(2), {
  detached: true,
  env: { ...process.env, AOW_HOST_ACTIVATION_DETACHED: '1' },
  stdio: ['ignore', 'inherit', 'inherit'],
});
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
