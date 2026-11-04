import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const generator = fileURLToPath(new URL('../launchd-service.mjs', import.meta.url));

for (const mode of ['launchagent', 'launchdaemon']) {
  for (const state of ['legacy', 'mismatched legacy', 'mismatched current', 'unmanaged', 'orphaned legacy', 'regular file']) {
    test(`${mode} configuration validates ${state} Node ownership before staging`, {
      skip: mode === 'launchdaemon' && process.getuid() === 0,
    }, t => {
      const home = mkdtempSync(join(tmpdir(), 'aow-launchd-node-'));
      t.after(() => rmSync(home, { recursive: true, force: true }));
      const runtime = join(home, 'runtime');
      const stage = join(home, 'stage');
      mkdirSync(runtime);
      mkdirSync(stage);
      const node = join(runtime, 'node');
      const marker = join(runtime, '.aow-runtime-daemon-node-target');
      const legacyMarker = join(runtime, '.aow-terminald-node-target');
      if (state === 'regular file') writeFileSync(node, 'user-owned node');
      else if (state !== 'orphaned legacy') symlinkSync('/old/node', node);
      if (state !== 'unmanaged') writeFileSync(legacyMarker, state === 'mismatched legacy' ? '/other/node\n' : '/old/node\n');
      if (state === 'mismatched current') writeFileSync(marker, '/other/node\n');
      const result = spawnSync(process.execPath,
        [generator, 'runtime-daemon', runtime, join(stage, 'service.plist'), process.execPath, mode],
        { env: { ...process.env, HOME: home }, encoding: 'utf8', timeout: 5000 });
      assert.ifError(result.error);
      if (state === 'legacy') {
        assert.equal(result.status, 0, result.stderr);
        assert.equal(readlinkSync(join(stage, 'node')), process.execPath);
        assert.equal(readFileSync(join(stage, '.aow-runtime-daemon-node-target'), 'utf8'), `${process.execPath}\n`);
        assert.equal(readlinkSync(node), '/old/node');
        assert.equal(readFileSync(legacyMarker, 'utf8'), '/old/node\n');
      } else {
        assert.notEqual(result.status, 0);
        assert.match(result.stderr, /unmanaged Node runtime|does not match installer marker|orphaned Node marker/);
        assert.equal(existsSync(join(stage, 'node')), false);
        assert.equal(existsSync(join(stage, 'service.plist')), false);
      }
      assert.equal(existsSync(marker), state === 'mismatched current');
    });
  }
}
