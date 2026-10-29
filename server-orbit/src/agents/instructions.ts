import { randomUUID } from 'node:crypto';
import { mkdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { HttpError } from '../errors.js';
import { readPackage, validatePackage, writePackage } from '../skills/files.js';

export class AgentInstructions {
  constructor(private stateDir: string) {}
  directory(org: string, agent: string, version: string) { return join(this.stateDir, 'agents', org, agent, 'instructions', version); }
  async prepare(org: string, agent: string, input: unknown) {
    const files = validatePackage(input, 'AGENTS.md');
    if (files.length > 100 || files.some(file => file.kind === 'file' && (!/\.md$/i.test(file.path) || file.encoding !== 'utf8')) || files.reduce((sum, file) => sum + (file.kind === 'file' ? Buffer.byteLength(file.content) : 0), 0) > 4 * 1024 * 1024) {
      throw new HttpError(422, 'invalid_instructions', '指令仅支持 Markdown 文件，最多 100 个条目、总计 4 MiB。');
    }
    const version = randomUUID(); const directory = this.directory(org, agent, version);
    await mkdir(join(directory, '..'), { recursive: true, mode: 0o700 });
    try { await writePackage(directory, files); }
    catch (error) { await rm(directory, { recursive: true, force: true }); throw error; }
    return { version, directory, files };
  }
  read(org: string, agent: string, version: string) { const directory = this.directory(org, agent, version); return readPackage(directory, directory, 'AGENTS.md'); }
  template(name: string, role: string, description: string) {
    return [{ path: 'AGENTS.md', kind: 'file', encoding: 'utf8', executable: false, content: `# ${name}\n\n## 角色职责\n\n${role || '定义此 Agent 的角色。'}\n\n${description || '补充职责范围。'}\n\n## 执行规则\n\n- 先理解任务与项目约定，再开始工作。\n- 按任务要求使用已启用的 Skills。\n\n## 输出要求\n\n- 清晰说明结果、验证情况和待处理事项。\n` }];
  }
}
