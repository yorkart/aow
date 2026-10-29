import { mkdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { transaction } from '../database.js';
import { HttpError } from '../errors.js';
import { readPackage, writePackage } from './files.js';
import type { Skills } from './service.js';

function component(value: string) {
  if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,79}$/.test(value)) throw new HttpError(422, 'invalid_identity', 'Agent 或运行标识无效。');
  return value;
}

interface PinnedSkill { skill_id: string; version_id: string; name: string; version: number }

/** Called by Agent configuration/runtime adapters, not by the Skills editor. */
export class SkillReferences {
  constructor(private skills: Skills) {}

  selected(org: string, agent: string): string[] {
    this.skills.checkOrg(org);
    component(agent);
    return (this.skills.database.prepare('SELECT skill_id FROM agent_skills WHERE org_id = ? AND agent_id = ? ORDER BY skill_id')
      .all(org, agent) as unknown as { skill_id: string }[]).map(item => item.skill_id);
  }

  set(org: string, agent: string, ids: string[]) {
    this.skills.checkOrg(org);
    component(agent);
    if (!Array.isArray(ids) || ids.length > 100 || ids.some(id => typeof id !== 'string')) throw new HttpError(422, 'invalid_references', '技能选择无效。');
    transaction(this.skills.database, () => {
      for (const id of ids) this.skills.get(org, id);
      this.skills.database.prepare('DELETE FROM agent_skills WHERE org_id = ? AND agent_id = ?').run(org, agent);
      const insert = this.skills.database.prepare('INSERT INTO agent_skills VALUES (?, ?, ?)');
      for (const id of new Set(ids)) insert.run(org, agent, id);
    });
    return this.selected(org, agent);
  }

  private existing(org: string, agent: string, run: string) {
    const row = this.skills.database.prepare('SELECT directory FROM skill_runs WHERE org_id = ? AND agent_id = ? AND run_id = ?').get(org, agent, run) as { directory: string } | undefined;
    if (!row) return undefined;
    const skills = this.skills.database.prepare(`SELECT r.skill_id, r.version_id, r.name, v.number AS version FROM skill_run_versions r
      JOIN skill_versions v ON v.id = r.version_id AND v.skill_id = r.skill_id
      WHERE r.org_id = ? AND r.agent_id = ? AND r.run_id = ? ORDER BY r.name`).all(org, agent, run) as unknown as PinnedSkill[];
    return { directory: row.directory, skills: skills.map(skill => ({ ...skill, directory: join(row.directory, skill.name) })) };
  }

  async pinRun(org: string, agent: string, run: string) {
    this.skills.checkOrg(org);
    component(agent);
    component(run);
    const existing = this.existing(org, agent, run);
    if (existing) return existing;
    const selected = this.selected(org, agent).map(id => this.skills.get(org, id));
    const directory = join(this.skills.stateDir, 'skill-runs', org, agent, run);
    await mkdir(join(this.skills.stateDir, 'skill-runs', org, agent), { recursive: true, mode: 0o700 });
    try { await mkdir(directory, { mode: 0o700 }); }
    catch { throw new HttpError(409, 'run_being_prepared', '此运行的技能目录正在准备，请稍后重试。'); }
    try {
      for (const skill of selected) {
        const version = this.skills.version(skill.current_version_id);
        await writePackage(join(directory, skill.name), await readPackage(version.directory));
      }
      transaction(this.skills.database, () => {
        this.skills.database.prepare('INSERT INTO skill_runs VALUES (?, ?, ?, ?, ?)').run(org, agent, run, directory, new Date().toISOString());
        const insert = this.skills.database.prepare('INSERT INTO skill_run_versions VALUES (?, ?, ?, ?, ?, ?)');
        for (const skill of selected) insert.run(org, agent, run, skill.id, skill.current_version_id, skill.name);
      });
    } catch (error) { await rm(directory, { recursive: true, force: true }); throw error; }
    return this.existing(org, agent, run)!;
  }
}
