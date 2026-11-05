import { createServer } from 'node:net';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

// Start the real execution node using the caller's child-process cleanup policy.
export async function hostNode(start, state) {
  const reservation = createServer();
  await new Promise(resolve => reservation.listen(0, '127.0.0.1', resolve));
  const address = `127.0.0.1:${reservation.address().port}`;
  await new Promise(resolve => reservation.close(resolve));
  const token = 'browser-host-fixture-token-0123456789abcdef';
  const tokenFile = join(state, 'node.token');
  await writeFile(tokenFile, token, { mode: 0o600 });
  const child = start('aow-host-daemon', ['--state-dir', state, '--listen', address,
    '--token-file', tokenFile, '--vt-node', process.execPath]);
  const url = `http://${address}`;
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${url}/v1/health`, {
        headers: { authorization: `Bearer ${token}` }, signal: AbortSignal.timeout(500),
      });
      await response.text();
      if (response.ok) return { url, tokenFile };
    } catch { /* Wait for the real listener, including first-run worker startup. */ }
    if (child.exitCode !== null) throw new Error(`Host exited: ${child.output ?? ''}`);
    await delay(30);
  }
  throw new Error(`Host startup timed out: ${child.output ?? ''}`);
}
