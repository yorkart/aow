import assert from 'node:assert/strict';
import { pbkdf2Sync } from 'node:crypto';

const cache = new Map();
export function credentials(password = 'test-password', username = 'admin') {
  const key = JSON.stringify([username, password]);
  if (!cache.has(key)) {
    const salt = Buffer.from('00112233445566778899aabbccddeeff', 'hex');
    cache.set(key, JSON.stringify({ version: 1, username, salt: salt.toString('hex'),
      password_hash: pbkdf2Sync(password, salt, 600000, 32, 'sha256').toString('hex') }) + '\n');
  }
  return cache.get(key);
}

export function assertCredentials(contents, password, username = 'admin') {
  const record = JSON.parse(contents);
  assert.deepEqual(Object.keys(record).sort(), ['password_hash', 'salt', 'username', 'version']);
  assert.equal(record.version, 1);
  assert.equal(record.username, username);
  assert.match(record.salt, /^[0-9a-f]{32}$/);
  assert.equal(record.password_hash, pbkdf2Sync(password, Buffer.from(record.salt, 'hex'), 600000, 32, 'sha256').toString('hex'));
}
