#!/usr/bin/env node

import { runWorker } from '../src/worker.mjs';

const controller = new AbortController();
let terminating = false;
const terminate = () => {
  if (terminating) return;
  terminating = true;
  controller.abort();
  process.stdin.destroy();
};

process.once('SIGTERM', terminate);
process.once('SIGINT', terminate);

try {
  await runWorker({
    input: process.stdin,
    output: process.stdout,
    // stdout is reserved exclusively for binary protocol frames.
    logger: console,
    signal: controller.signal,
  });
} catch (error) {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exitCode = 1;
}
