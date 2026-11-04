import { spawn } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { createInterface } from 'node:readline';

// Transparent JSON-RPC recorder for the opt-in live steering regression.
// Output is raw local evidence. Anonymize content and identities before committing fixtures.
const [output, command, ...args] = process.argv.slice(2);
const child = spawn(command, args, { stdio: ['pipe', 'pipe', 'inherit'] });
const record = (direction, line) => appendFileSync(output, `${JSON.stringify({ direction, message: JSON.parse(line) })}\n`);
createInterface({ input: process.stdin }).on('line', line => {
  record('client', line);
  child.stdin.write(`${line}\n`);
}).on('close', () => child.stdin.end());
createInterface({ input: child.stdout }).on('line', line => {
  record('agent', line);
  process.stdout.write(`${line}\n`);
});
child.on('error', error => { process.stderr.write(`${error.message}\n`); process.exitCode = 1; });
child.on('close', code => process.exit(code ?? 1));
