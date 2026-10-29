export interface SkillFile { path: string; kind: 'file'; encoding: 'utf8' | 'base64'; content: string; executable: boolean }
export interface SkillDirectory { path: string; kind: 'directory' }
export type SkillEntry = SkillFile | SkillDirectory;
export interface Skill {
  id: string; org_id: string; name: string; description: string; version: number;
  current_version_id: string; updated_at: string; agent_count: number; content_hash: string;
  source_project_id: string | null; source_path: string | null;
}
export interface SkillDetail extends Skill { files: SkillEntry[]; agents: { agent_id: string }[] }
export interface Project { id: string; name: string; directory: string | null }
export interface Candidate { path: string; name: string; description: string }
export interface ScanResult { project: Project; skills: Candidate[]; warnings: string[] }
