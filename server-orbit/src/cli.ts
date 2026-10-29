import { createInterface } from 'node:readline/promises';
import { Writable } from 'node:stream';
import { setAccount } from './auth.js';
import { configuration } from './config.js';
import { account, openDatabase } from './database.js';

const command = process.argv[2];
if (command !== 'setup' && command !== 'account') throw new Error('Usage: cli.js setup|account');
const database = openDatabase(configuration().stateDir);
try {
  if (command !== 'setup' || !account(database)) {
    let username = process.env.ORBIT_USERNAME;
    let password = process.env.ORBIT_PASSWORD;
    if (username === undefined || password === undefined) {
      if (!process.stdin.isTTY || !process.stdout.isTTY) throw new Error('请在终端运行账户配置，或同时设置 ORBIT_USERNAME 和 ORBIT_PASSWORD。');
      let hidden = false;
      const output = new Writable({ write(chunk, _encoding, callback) { if (!hidden) process.stdout.write(chunk); callback(); } });
      const prompt = createInterface({ input: process.stdin, output, terminal: true });
      try {
        username = await prompt.question('Orbit 用户名：');
        process.stdout.write('Orbit 密码：');
        hidden = true;
        password = await prompt.question('');
        process.stdout.write('\n确认密码：');
        const confirmation = await prompt.question('');
        process.stdout.write('\n');
        if (password !== confirmation) throw new Error('两次输入的密码不一致。');
      } finally {
        hidden = false;
        prompt.close();
      }
    }
    await setAccount(database, username, password);
    console.log('Orbit 登录账户已保存。');
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : '账户配置失败。');
  process.exitCode = 1;
} finally {
  database.close();
}
