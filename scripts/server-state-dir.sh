#!/usr/bin/env bash

# Shared by the installer and the packaged aow command.
server_state_dir() {
    node --input-type=module - <<'JS'
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
let configured;
try {
    for (const line of readFileSync(join(process.env.HOME, '.config/aow/server.env'), 'utf8').split(/\r?\n/)) {
        const match = line.match(/^\s*AOW_SERVER_STATE_DIR\s*=\s*(.*?)\s*$/);
        if (match) configured = match[1].replace(/^(['"])(.*)\1$/, '$2');
    }
} catch (error) { if (error.code !== 'ENOENT') throw error; }
process.stdout.write(process.env.AOW_SERVER_STATE_DIR || configured || process.env.AOW_STATE_DIR
    || join(process.env.XDG_STATE_HOME || join(process.env.HOME, '.local/state'), 'aow'));
JS
}
