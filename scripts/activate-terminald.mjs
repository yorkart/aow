import { spawn } from 'node:child_process';

// The user has already confirmed the restart. A new session lets activation
// finish even when stopping terminald closes the terminal running the installer.
// Normally the caller still waits and receives the actual activation result.
const child = spawn('/bin/sh', process.argv.slice(2), {
  detached: true,
  stdio: ['ignore', 'inherit', 'inherit'],
});
child.on('error', error => { console.error(error.message); process.exitCode = 1; });
child.on('exit', code => { process.exitCode = code ?? 1; });
